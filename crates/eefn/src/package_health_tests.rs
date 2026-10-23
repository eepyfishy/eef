use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
struct Fake {
    calls: AtomicUsize,
    behavior: u8,
    seen: Mutex<Vec<Vec<String>>>,
    paths: Mutex<Vec<PathBuf>>,
}
impl Fake {
    fn new(behavior: u8) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            behavior,
            seen: Mutex::new(vec![]),
            paths: Mutex::new(vec![]),
        }
    }
}
impl Runner for Fake {
    async fn run(
        &self,
        path: &Path,
        args: &[String],
        _: &Path,
        _: &Args,
    ) -> std::result::Result<Captured, Failure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(args.into());
        self.paths.lock().unwrap().push(path.to_path_buf());
        if self.behavior == 1 {
            return Err(Failure::Deadline);
        }
        if self.behavior == 2 {
            return Err(Failure::Output);
        }
        if self.behavior == 3 || (self.behavior == 6 && path.ends_with("llama-server.exe")) {
            std::fs::write(path, b"changed during owned probe").unwrap();
        }
        if self.behavior == 7 {
            std::fs::write(
                path.parent().unwrap().parent().unwrap().join("current.txt"),
                "99.0.0",
            )
            .unwrap();
        }
        if self.behavior == 4 {
            return Ok(Captured {
                stdout: b"PRIVATE invalid output".to_vec(),
                stderr: b"PRIVATE trace".to_vec(),
            });
        }
        if path.ends_with("llama-server.exe") {
            return Ok(Captured {
                stdout: vec![],
                stderr:
                    b"version: 0.3.0-dev (build 10621, commit c1d0e7a00)\nbuilt with compiler\n"
                        .to_vec(),
            });
        }
        let modules: Vec<String> = serde_json::from_str(args.last().unwrap()).unwrap();
        let body = json!({"schema_version":1,"version":[3,11,9],"bits":64,"modules":modules.iter().map(|name|json!({"name":name,"imported":self.behavior!=5})).collect::<Vec<_>>()});
        Ok(Captured {
            stdout: serde_json::to_vec(&body).unwrap(),
            stderr: vec![],
        })
    }
}
fn fixture(program: &str) -> (tempfile::TempDir, Context) {
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join(format!("{program}.exe"));
    std::fs::write(&exe, b"fixture app").unwrap();
    std::fs::create_dir(root.path().join("python")).unwrap();
    std::fs::write(root.path().join("python/python.exe"), b"fixture python").unwrap();
    std::fs::create_dir(root.path().join("tools")).unwrap();
    std::fs::write(root.path().join("tools/llama-server.exe"), b"fixture llama").unwrap();
    let marker = json!({"name":program,"version":crate::VERSION,"architecture":"x86_64-windows","python":"3.11.9-embedded","llamacpp":"b10621"});
    std::fs::write(
        root.path().join("bundle.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    let ctx = Context::for_executable(exe, program, crate::VERSION).unwrap();
    (root, ctx)
}
fn probing() -> Args {
    Args {
        probe: true,
        ..Args::default()
    }
}

#[tokio::test]
async fn doctor_default_is_inspection_only_without_files_or_source_fallback() {
    for role in ["eef", "eefn"] {
        let (root, ctx) = fixture(role);
        std::fs::remove_file(root.path().join("python/python.exe")).unwrap();
        let count = std::fs::read_dir(root.path()).unwrap().count();
        let fake = Fake::new(0);
        let report = execute_with(&ctx, &Args::default(), &fake).await.unwrap();
        assert_eq!(report["success"], true);
        assert_eq!(report["probes_requested"], false);
        assert_eq!(report["all_requested_probes_passed"], Value::Null);
        assert_eq!(report["probe_identity_unchanged"], Value::Null);
        assert_eq!(report["probes"][0]["file_state"], "missing");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), count);
    }
}
#[tokio::test]
async fn doctor_role_scopes_and_explicit_media_are_not_model_or_feature_health() {
    for role in ["eef", "eefn"] {
        let (_root, ctx) = fixture(role);
        let fake = Fake::new(0);
        let args = Args {
            media: role == "eefn",
            ..probing()
        };
        let report = execute_with(&ctx, &args, &fake).await.unwrap();
        assert_eq!(report["all_requested_probes_passed"], true);
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            if role == "eef" { 1 } else { 3 }
        );
        for key in [
            "model_inference_checked",
            "running_service_inspected",
            "permissions_changed",
            "models_changed",
            "repair_requested",
            "restart_requested",
            "automatically_retried",
        ] {
            assert_eq!(report[key], false);
        }
        assert_eq!(report["system_ready"], Value::Null);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(&seen[0][..4], &["-I", "-B", "-X", "utf8"]);
        if role == "eefn" {
            assert_eq!(seen[1], vec!["--version"]);
        }
    }
}
#[tokio::test]
async fn doctor_missing_runtime_never_launches_or_borrows_path() {
    let (root, ctx) = fixture("eef");
    std::fs::remove_file(root.path().join("python/python.exe")).unwrap();
    let fake = Fake::new(0);
    let report = execute_with(&ctx, &probing(), &fake).await.unwrap();
    assert_eq!(report["success"], false);
    assert_eq!(
        report["probes"][0]["failure_code"],
        "bundled_runtime_missing"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn doctor_invalid_metadata_and_options_refuse_before_any_process() {
    let (root, ctx) = fixture("eefn");
    let fake = Fake::new(0);
    for args in [
        Args {
            media: true,
            ..Args::default()
        },
        Args {
            timeout_seconds: 0,
            ..probing()
        },
        Args {
            max_job_memory_mib: 2049,
            ..probing()
        },
    ] {
        assert!(execute_with(&ctx, &args, &fake).await.is_err());
    }
    for bytes in [
        b"broken marker".to_vec(),
        vec![b' '; 16385],
        serde_json::to_vec(
            &json!({"name":"eef","version":crate::VERSION,"architecture":"x86_64-windows"}),
        )
        .unwrap(),
    ] {
        std::fs::write(root.path().join("bundle.json"), bytes).unwrap();
        assert!(execute_with(&ctx, &probing(), &fake).await.is_err());
    }
    std::fs::remove_file(root.path().join("bundle.json")).unwrap();
    assert!(execute_with(&ctx, &probing(), &fake).await.is_err());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn doctor_changed_probe_identity_invalidates_success_without_replay() {
    let (_root, ctx) = fixture("eefn");
    let fake = Fake::new(3);
    let report = execute_with(
        &ctx,
        &Args {
            media: true,
            ..probing()
        },
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(report["probe_identity_unchanged"], false);
    assert_eq!(report["all_requested_probes_passed"], false);
    for row in report["probes"].as_array().unwrap() {
        assert_eq!(row["state"], "unconfirmed");
        assert_eq!(row["passed"], Value::Null);
    }
}
#[tokio::test]
async fn doctor_later_identity_change_invalidates_earlier_passing_rows() {
    let (_root, ctx) = fixture("eefn");
    let fake = Fake::new(6);
    let report = execute_with(
        &ctx,
        &Args {
            media: true,
            ..probing()
        },
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(report["success"], false);
    for row in report["probes"].as_array().unwrap() {
        assert_eq!(row["state"], "unconfirmed");
        assert_eq!(row["passed"], Value::Null);
    }
}
#[tokio::test]
async fn doctor_deadline_output_and_invalid_replies_are_unconfirmed_or_failed_without_raw_output() {
    for behavior in [1, 2, 4, 5] {
        let (_root, ctx) = fixture("eef");
        let fake = Fake::new(behavior);
        let report = execute_with(&ctx, &probing(), &fake).await.unwrap();
        assert_eq!(report["success"], false);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        assert!(!report.to_string().contains("PRIVATE"));
        assert!(render(&report).contains("not system readiness"));
    }
}
#[test]
fn doctor_python_reply_is_strict_versioned_and_import_specific() {
    let good = json!({"schema_version":1,"version":[3,11,9],"bits":64,"modules":[{"name":"json","imported":true}]});
    let bytes = serde_json::to_vec(&good).unwrap();
    assert_eq!(
        python_reply(&bytes, &["json"], Some("3.11.9-embedded")).unwrap()["passed"],
        true
    );
    assert_eq!(
        python_reply(&bytes, &["json"], Some("3.12.0-embedded")).unwrap()["passed"],
        false
    );
    assert!(python_reply(&bytes, &["ctypes"], None).is_err());
    for (field, value) in [
        ("schema_version", json!(2)),
        ("bits", json!(32)),
        ("version", json!([3, 11, 9, 0])),
        ("grant", json!(true)),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(python_reply(&serde_json::to_vec(&bad).unwrap(), &["json"], None).is_err());
    }
    assert!(python_reply(&vec![b' '; LIMIT + 1], &[], None).is_err());
    assert!(
        python_reply(
            br#"{"schema_version":1,"schema_version":1,"version":[3,11,9],"bits":64,"modules":[]}"#,
            &[],
            None
        )
        .is_err()
    );
}
#[test]
fn doctor_llama_reply_supports_actual_and_legacy_formats_not_arbitrary_logs() {
    for text in [
        "version: 0.3.0-dev (build 10621, commit c1d0e7a00)\r\nbuilt with compiler\r\n",
        "version: 10621 (c1d0e7a00)\n",
    ] {
        assert_eq!(
            llama_reply(&[], text.as_bytes(), Some("b10621")).unwrap()["passed"],
            true
        );
        assert_eq!(
            llama_reply(text.as_bytes(), &[], Some("b10622")).unwrap()["passed"],
            false
        );
        assert!(llama_reply(text.as_bytes(), text.as_bytes(), None).is_err());
    }
    assert!(llama_reply(b"PRIVATE arbitrary log", &[], None).is_err());
    assert!(llama_reply(&[255], &[], None).is_err());
}
#[tokio::test]
async fn doctor_pending_selection_does_not_become_live_health() {
    let (root, ctx) = fixture("eef");
    std::fs::write(root.path().join("current.txt"), "99.0.0").unwrap();
    let fake = Fake::new(0);
    let report = execute_with(&ctx, &probing(), &fake).await.unwrap();
    assert_eq!(report["selection_pending"], true);
    assert_eq!(report["scope"], "invoked_metadata_only");
    assert_eq!(report["selected_target_resolved"], false);
    assert_eq!(report["success"], false);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        report["probes"][0]["failure_code"],
        "selected_bundle_unavailable"
    );
    assert_eq!(report["running_service_inspected"], false);
    assert_eq!(report["system_ready"], Value::Null);
    assert!(render(&report).contains("selection differs or is unknown"));
}
#[tokio::test]
async fn doctor_selected_generation_uses_its_runtimes_not_old_root_or_app_launch() {
    let (root, ctx) = fixture("eefn");
    let version = crate::VERSION;
    let slot =
        crate::bundle_selection::repair_slot(version, &uuid::Uuid::new_v4().to_string()).unwrap();
    let selected = root.path().join("versions").join(&slot);
    std::fs::create_dir_all(selected.join("python")).unwrap();
    std::fs::create_dir_all(selected.join("tools")).unwrap();
    for name in [
        "eefn.exe",
        "bundle.json",
        "python/python.exe",
        "tools/llama-server.exe",
    ] {
        std::fs::copy(root.path().join(name), selected.join(name)).unwrap();
    }
    std::fs::remove_file(root.path().join("python/python.exe")).unwrap();
    std::fs::write(root.path().join("current.txt"), version).unwrap();
    crate::bundle_selection::write(
        root.path(),
        &crate::bundle_selection::BundleSelection {
            schema_version: 1,
            version: version.into(),
            slot,
            previous_slot: Some("$root".into()),
        },
    )
    .unwrap();
    let fake = Fake::new(0);
    let report = execute_with(&ctx, &probing(), &fake).await.unwrap();
    assert_eq!(report["scope"], "selected_package");
    assert_eq!(report["selected_target_resolved"], true);
    assert_eq!(report["all_requested_probes_passed"], true);
    assert_eq!(report["target_application_launch_checked"], false);
    let canonical_selected = selected.canonicalize().unwrap();
    assert!(
        fake.paths
            .lock()
            .unwrap()
            .iter()
            .all(|p| p.starts_with(&canonical_selected))
    );
}
#[tokio::test]
async fn doctor_selection_change_retires_observations_and_never_probes_second_target() {
    let (_root, ctx) = fixture("eefn");
    let fake = Fake::new(7);
    let report = execute_with(&ctx, &probing(), &fake).await.unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(report["probe_identity_unchanged"], false);
    assert_eq!(report["success"], false);
    assert!(
        report["probes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["state"] == "unconfirmed")
    );
}
#[test]
fn doctor_owner_flags_are_explicit_and_bounded() {
    use clap::Parser;
    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }
    assert!(!Cli::try_parse_from(["doctor"]).unwrap().args.probe);
    assert!(Cli::try_parse_from(["doctor", "--media"]).is_err());
    assert!(Cli::try_parse_from(["doctor", "--probe", "--media"]).is_ok());
    for (flag, value) in [
        ("--timeout-seconds", "0"),
        ("--timeout-seconds", "31"),
        ("--max-job-memory-mib", "63"),
        ("--max-job-memory-mib", "2049"),
    ] {
        assert!(Cli::try_parse_from(["doctor", flag, value]).is_err());
    }
}
#[test]
fn doctor_probe_environment_and_working_directory_are_package_scoped() {
    let (_root, ctx) = fixture("eefn");
    let runtime = ctx.bundle.join("python");
    let exe = runtime.join("python.exe");
    let before = std::env::var_os("PATH");
    let cmd = probe_command(&exe, &["-I".into()], &ctx.bundle).unwrap();
    assert_eq!(cmd.as_std().get_current_dir(), Some(runtime.as_path()));
    let env = cmd.as_std().get_envs().collect::<Vec<_>>();
    assert!(
        env.iter()
            .any(|(name, value)| *name == std::ffi::OsStr::new("PATH")
                && *value == Some(runtime.as_os_str()))
    );
    for (name, _) in env {
        assert!(
            [
                "PATH",
                "SystemRoot",
                "WINDIR",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "LOCALAPPDATA",
                "APPDATA",
                "COMSPEC",
                "PATHEXT"
            ]
            .iter()
            .any(|key| name == std::ffi::OsStr::new(key))
        );
    }
    assert_eq!(std::env::var_os("PATH"), before);
    assert!(probe_command(&ctx.install.join("outside/runtime.exe"), &[], &runtime).is_err());
}
#[cfg(windows)]
#[tokio::test]
async fn doctor_loader_error_mode_is_restored_before_wait() {
    use windows_sys::Win32::System::Diagnostics::Debug::GetErrorMode;
    let original = unsafe { GetErrorMode() };
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, tree) = spawn(&mut cmd, 512).unwrap();
    assert_eq!(unsafe { GetErrorMode() }, original);
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    tree.terminate_and_observe_until(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
}
