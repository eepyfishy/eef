use super::*;
use std::sync::atomic::AtomicUsize;

struct FakeSource {
    calls: AtomicUsize,
    plan: String,
    behavior: u8,
}
impl FakeSource {
    fn new(behavior: u8) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            plan: uuid::Uuid::new_v4().to_string(),
            behavior,
        }
    }
}
impl SourceExecutor for FakeSource {
    async fn execute(
        &self,
        ctx: &Context,
        args: &crate::package_repair::RepairArgs,
        fence: &Fence,
    ) -> Result<Value> {
        drop(fence.authorize()?);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if args.check {
            if self.behavior == 1 {
                configure(ctx, Mode::Off, false, None, None)?;
                drop(fence.authorize()?);
            }
            if self.behavior == 2 {
                for path in &fence.record.missing {
                    let path = ctx.bundle().join(path);
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    std::fs::write(path, b"restored fixture file")?;
                }
                drop(fence.authorize()?);
            }
            if self.behavior == 3 {
                return Ok(json!({"success":false}));
            }
            Ok(json!({"success":true,"plan":{"id":self.plan}}))
        } else {
            assert_eq!(args.approve.as_deref(), Some(self.plan.as_str()));
            if fence.policy.mode == Mode::Prompt {
                assert!(fence.record.owner_approved);
            }
            if self.behavior == 4 {
                return Ok(json!({"success":false,"outcome_unknown":true}));
            }
            Ok(json!({"success":true,"outcome_unknown":false}))
        }
    }
}

fn first_record(ctx: &Context) -> Record {
    load_state(ctx).unwrap().0.records[0].clone()
}
fn admit(ctx: &Context, phase: Phase) -> (Record, File, File) {
    let policy = load_policy(ctx).unwrap().0;
    let serial = runner_lease(ctx).unwrap();
    let (mut state, before) = load_state(ctx).unwrap();
    let now = crate::protocol::now_ms();
    let record = Record {
        id: uuid::Uuid::new_v4().to_string(),
        grant: policy.clone(),
        policy_id: policy.policy_id.unwrap(),
        revision: policy.revision,
        phase,
        plan_id: if phase == Phase::Checking {
            None
        } else {
            Some(uuid::Uuid::new_v4().to_string())
        },
        missing: missing(ctx, &policy.binding.unwrap().components).unwrap(),
        owner_approved: false,
        outcome_unknown: false,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let op = operation_lease(ctx, &record.id, true).unwrap();
    state.records.push(record.clone());
    write_checked(
        &ctx.state(),
        before.as_deref(),
        &serde_json::to_vec(&state).unwrap(),
        STATE_MAX,
    )
    .unwrap();
    (record, serial, op)
}

struct Retired(Arc<AtomicBool>);
impl Drop for Retired {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct WaitingSource {
    started: tokio::sync::Notify,
    retired: Arc<AtomicBool>,
}
impl WaitingSource {
    fn new() -> Self {
        Self {
            started: tokio::sync::Notify::new(),
            retired: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl SourceExecutor for WaitingSource {
    async fn execute(
        &self,
        _ctx: &Context,
        _args: &crate::package_repair::RepairArgs,
        fence: &Fence,
    ) -> Result<Value> {
        drop(fence.authorize()?);
        let _retired = Retired(self.retired.clone());
        self.started.notify_one();
        std::future::pending().await
    }
}

fn fixture(role: &str, version: &str) -> (tempfile::TempDir, Context) {
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join(format!("{role}.exe"));
    std::fs::write(&exe, b"owned fixture build").unwrap();
    std::fs::write(
        root.path().join("bundle.json"),
        serde_json::to_vec(&json!({"name":role,"version":version})).unwrap(),
    )
    .unwrap();
    std::fs::write(root.path().join(format!("{role}.repair-source.json")),serde_json::to_vec(&json!({"schema_version":1,"program":role,"version":version,"manifest_url":"https://publisher.invalid/exact.json"})).unwrap()).unwrap();
    (root, Context::for_executable(exe, role, version).unwrap())
}

#[test]
fn package_setup_default_inspection_does_not_create_authority_or_files() {
    let (root, ctx) = fixture("eefn", "1.0.0");
    let count = std::fs::read_dir(root.path()).unwrap().count();
    let report = show(&ctx);
    assert_eq!(report["success"], true);
    assert_eq!(report["policy"]["mode"], "off");
    assert_eq!(report["runtime_health_checked"], false);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), count);
}

#[test]
fn package_setup_auto_requires_distinct_consent_and_stable_build() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    assert!(configure(&ctx, Mode::Auto, false, None, None).is_err());
    assert!(configure(&ctx, Mode::Prompt, true, None, None).is_err());
    assert!(!ctx.policy().exists());
    let (_root, alpha) = fixture("eefn", "1.0.0-alpha.1");
    assert!(configure(&alpha, Mode::Auto, true, None, None).is_err());
    assert!(!alpha.policy().exists());
    assert!(configure(&alpha, Mode::Prompt, false, None, None).is_ok());
}

#[test]
fn package_setup_policy_cas_rotation_and_explicit_opt_out_are_independent_of_history() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Prompt, false, None, Some(0)).unwrap();
    let first = load_policy(&ctx).unwrap().0;
    assert!(configure(&ctx, Mode::Auto, true, None, Some(0)).is_err());
    assert_eq!(load_policy(&ctx).unwrap().0, first);
    std::fs::write(ctx.state(), b"unsupported state").unwrap();
    configure(&ctx, Mode::Off, false, None, Some(1)).unwrap();
    let off = load_policy(&ctx).unwrap().0;
    assert_eq!(off.mode, Mode::Off);
    assert_ne!(off.policy_id, first.policy_id);
    assert_eq!(std::fs::read(ctx.state()).unwrap(), b"unsupported state");
}

#[test]
fn package_setup_automatic_public_source_rejects_private_query_credentials_and_fragments() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    for source in [
        "http://publisher.invalid/a",
        "https://user:secret@publisher.invalid/a",
        "https://publisher.invalid/a?token=SECRET",
        "https://publisher.invalid/a#part",
    ] {
        assert!(configure(&ctx, Mode::Auto, true, Some(source), None).is_err());
        assert!(!ctx.policy().exists());
    }
}

#[tokio::test]
async fn package_setup_prompt_retains_exact_plan_until_explicit_approval_without_replay() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Prompt, false, None, None).unwrap();
    let source = FakeSource::new(0);
    drive(&ctx, None, Control::local(), &source).await.unwrap();
    let record = first_record(&ctx);
    assert_eq!(record.phase, Phase::WaitingApproval);
    assert_eq!(record.plan_id.as_deref(), Some(source.plan.as_str()));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    drive(&ctx, None, Control::local(), &source).await.unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    drive(&ctx, Some(&record.id), Control::local(), &source)
        .await
        .unwrap();
    assert_eq!(first_record(&ctx).phase, Phase::Selected);
    assert!(first_record(&ctx).owner_approved);
    assert!(
        drive(&ctx, Some(&record.id), Control::local(), &source)
            .await
            .is_err()
    );
    drive(&ctx, None, Control::local(), &source).await.unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn package_setup_auto_is_one_attempt_per_explicit_policy_identity_and_preserves_owner_data() {
    for role in ["eef", "eefn"] {
        let (root, ctx) = fixture(role, "1.0.0");
        let owner = root.path().join("config.json");
        std::fs::write(&owner, b"owner config sentinel").unwrap();
        configure(&ctx, Mode::Auto, true, None, None).unwrap();
        let source = FakeSource::new(0);
        drive(&ctx, None, Control::local(), &source).await.unwrap();
        assert_eq!(first_record(&ctx).phase, Phase::Selected);
        assert!(!first_record(&ctx).owner_approved);
        drive(&ctx, None, Control::local(), &source).await.unwrap();
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        let old = first_record(&ctx).policy_id;
        configure(&ctx, Mode::Auto, true, None, Some(1)).unwrap();
        let next = FakeSource::new(0);
        drive(&ctx, None, Control::local(), &next).await.unwrap();
        let state = load_state(&ctx).unwrap().0;
        assert_eq!(state.records.len(), 2);
        assert_ne!(state.records[1].policy_id, old);
        assert_eq!(std::fs::read(owner).unwrap(), b"owner config sentinel");
        assert!(!ctx.bundle().join("current.txt").exists());
        // Fake executor drives lifecycle only, not real artifact publication.
    }
}

#[tokio::test]
async fn package_setup_metadata_failure_revocation_and_resolved_causes_never_replay() {
    for behavior in [1, 2, 3] {
        let (_root, ctx) = fixture("eefn", "1.0.0");
        configure(&ctx, Mode::Auto, true, None, None).unwrap();
        let source = FakeSource::new(behavior);
        drive(&ctx, None, Control::local(), &source).await.unwrap();
        let record = first_record(&ctx);
        assert!(matches!(record.phase, Phase::Interrupted | Phase::Failed));
        drive(&ctx, None, Control::local(), &source).await.unwrap();
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn package_setup_artifact_failure_preserves_uncertainty_instead_of_claiming_no_effect() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let source = FakeSource::new(4);
    let report = drive(&ctx, None, Control::local(), &source).await.unwrap();
    assert_eq!(first_record(&ctx).phase, Phase::Failed);
    assert!(first_record(&ctx).outcome_unknown);
    assert_eq!(report["records"][0]["outcome_unresolved"], true);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn package_setup_alpha_and_retired_context_never_admit_or_contact_a_source() {
    let (_root, ctx) = fixture("eefn", "1.0.0-alpha.1");
    configure(&ctx, Mode::Prompt, false, None, None).unwrap();
    let source = FakeSource::new(0);
    assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
    assert!(!ctx.state().exists());
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    let (_root, stable) = fixture("eefn", "1.0.0");
    configure(&stable, Mode::Auto, true, None, None).unwrap();
    let control = Control::local();
    control.alive.store(false, Ordering::Release);
    assert!(drive(&stable, None, control, &source).await.is_err());
    assert!(!stable.state().exists());
}

#[tokio::test]
async fn package_setup_changed_build_pending_selection_and_healthy_files_do_not_trigger_a_source() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let source = FakeSource::new(0);
    std::fs::write(&ctx.executable, b"changed image").unwrap();
    assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
    assert_eq!(
        show(&ctx)["policy_applicability"],
        "reauthorization_required"
    );
    std::fs::write(&ctx.executable, b"owned fixture build").unwrap();
    std::fs::write(ctx.install.join("current.txt"), "99.0.0").unwrap();
    assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
    std::fs::remove_file(ctx.install.join("current.txt")).unwrap();
    for path in &load_policy(&ctx).unwrap().0.binding.unwrap().components {
        let path = ctx.bundle().join(path);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"present").unwrap();
        }
    }
    drive(&ctx, None, Control::local(), &source).await.unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    assert!(load_state(&ctx).unwrap().0.records.is_empty());
}

#[test]
fn package_setup_component_selection_excludes_models_settings_and_deep_custom_data() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    let inventory = json!([
        {"path":"config.json"},{"path":"models/weight.gguf"},{"path":"python/Lib/site-packages/private.py"},
        {"path":"python/python311.dll"},{"path":"python/_ssl.pyd"},{"path":"tools/ggml-base.dll"},{"path":"libunwind.dll"}]);
    std::fs::write(
        ctx.bundle().join("files.sha256.json"),
        serde_json::to_vec(&inventory).unwrap(),
    )
    .unwrap();
    let paths = components(&ctx).unwrap();
    for included in [
        "python/python311.dll",
        "python/_ssl.pyd",
        "tools/ggml-base.dll",
        "libunwind.dll",
    ] {
        assert!(paths.iter().any(|p| p == included));
    }
    for excluded in [
        "config.json",
        "models/weight.gguf",
        "python/Lib/site-packages/private.py",
    ] {
        assert!(!paths.iter().any(|p| p == excluded));
    }
    std::fs::write(
        ctx.bundle().join("files.sha256.json"),
        br#"[{"path":"python/../escape.dll"}]"#,
    )
    .unwrap();
    assert!(components(&ctx).is_err());
}

#[test]
fn package_setup_live_and_interrupted_leases_are_per_attempt_and_inspection_never_rewrites() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let (old, serial, op) = admit(&ctx, Phase::Checking);
    let before = std::fs::read(ctx.state()).unwrap();
    assert_eq!(show(&ctx)["records"][0]["lease"], "active");
    drop(op);
    drop(serial);
    assert_eq!(show(&ctx)["records"][0]["outcome_unresolved"], true);
    assert_eq!(std::fs::read(ctx.state()).unwrap(), before);
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let (_new, _serial, _op) = admit(&ctx, Phase::Checking);
    let report = show(&ctx);
    assert_eq!(report["records"][0]["record"]["id"], old.id);
    assert_eq!(report["records"][0]["lease"], "released");
    assert_eq!(report["records"][1]["lease"], "active");
}

#[tokio::test]
async fn package_setup_released_in_progress_record_cannot_resume_after_restart() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let (_record, serial, op) = admit(&ctx, Phase::Checking);
    drop(op);
    drop(serial);
    let before = std::fs::read(ctx.state()).unwrap();
    let source = FakeSource::new(0);
    let report = drive(&ctx, None, Control::local(), &source).await.unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    assert_eq!(report["records"][0]["outcome_unresolved"], true);
    assert_eq!(std::fs::read(ctx.state()).unwrap(), before);
}

#[test]
fn package_setup_final_policy_guard_blocks_a_concurrent_save_until_commit_scope_retires() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let (record, _serial, _op) = admit(&ctx, Phase::Approving);
    let policy = load_policy(&ctx).unwrap().0;
    let fence = Fence {
        ctx: ctx.clone(),
        policy: policy.clone(),
        record,
        artifact: true,
        control: Control::local(),
    };
    let guard = fence.authorize().unwrap();
    assert!(configure(&ctx, Mode::Off, false, None, None).is_err());
    assert_eq!(load_policy(&ctx).unwrap().0, policy);
    drop(guard);
    configure(&ctx, Mode::Off, false, None, None).unwrap();
    assert!(fence.authorize().is_err());
}

#[tokio::test]
async fn package_setup_revocation_retires_the_owned_source_future_before_checkpoint() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let source = Arc::new(WaitingSource::new());
    let copy = source.clone();
    let target = ctx.clone();
    let task =
        tokio::spawn(async move { drive(&target, None, Control::local(), copy.as_ref()).await });
    tokio::time::timeout(Duration::from_secs(2), source.started.notified())
        .await
        .unwrap();
    configure(&ctx, Mode::Off, false, None, None).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(source.retired.load(Ordering::SeqCst));
    assert_eq!(first_record(&ctx).phase, Phase::Interrupted);
    assert_eq!(show(&ctx)["records"][0]["lease"], "released");
}

#[tokio::test]
async fn package_setup_shutdown_watch_and_unavailable_policy_retire_without_replay() {
    for corrupt in [false, true] {
        let (_root, ctx) = fixture("eefn", "1.0.0");
        configure(&ctx, Mode::Auto, true, None, None).unwrap();
        let source = Arc::new(WaitingSource::new());
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let control = Control {
            alive: Arc::new(AtomicBool::new(true)),
            stopped: Some(stopped),
        };
        let target = ctx.clone();
        let copy = source.clone();
        let task = tokio::spawn(async move { drive(&target, None, control, copy.as_ref()).await });
        tokio::time::timeout(Duration::from_secs(2), source.started.notified())
            .await
            .unwrap();
        if corrupt {
            std::fs::write(ctx.policy(), b"future/unknown policy").unwrap();
        } else {
            stop.send(true).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(source.retired.load(Ordering::SeqCst));
        assert_eq!(first_record(&ctx).phase, Phase::Interrupted);
    }
}

#[tokio::test]
async fn package_setup_damaged_future_oversized_or_missing_history_never_resets() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Auto, true, None, None).unwrap();
    let source = FakeSource::new(0);
    for bytes in [
        b"bad state".to_vec(),
        br#"{"schema_version":99,"program":"eefn","records":[]}"#.to_vec(),
        vec![b' '; STATE_MAX + 1],
    ] {
        std::fs::write(ctx.state(), &bytes).unwrap();
        assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
        assert_eq!(std::fs::read(ctx.state()).unwrap(), bytes);
    }
    std::fs::remove_file(ctx.state()).unwrap();
    let _serial = runner_lease(&ctx).unwrap();
    drop(_serial);
    std::fs::remove_file(ctx.state()).unwrap();
    assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
    assert!(!ctx.state().exists());
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn package_setup_original_grant_survives_opt_out_and_invalid_snapshots_refuse() {
    let (_root, ctx) = fixture("eefn", "1.0.0");
    configure(&ctx, Mode::Prompt, false, None, None).unwrap();
    let original = load_policy(&ctx).unwrap().0;
    drive(&ctx, None, Control::local(), &FakeSource::new(0))
        .await
        .unwrap();
    configure(&ctx, Mode::Off, false, None, Some(1)).unwrap();
    assert_eq!(first_record(&ctx).grant, original);
    assert_eq!(show(&ctx)["policy"]["mode"], "off");
    assert_eq!(
        show(&ctx)["records"][0]["record"]["grant"]["mode"],
        "prompt"
    );
    let original_bytes = std::fs::read(ctx.state()).unwrap();
    for field in ["policy_id", "revision", "source", "component"] {
        let mut state: State = serde_json::from_slice(&original_bytes).unwrap();
        let record = &mut state.records[0];
        match field {
            "policy_id" => record.grant.policy_id = Some(uuid::Uuid::new_v4().to_string()),
            "revision" => record.grant.revision += 1,
            "source" => {
                record.grant.binding.as_mut().unwrap().public_source =
                    "https://publisher.invalid/metadata?token=SECRET".into()
            }
            "component" => record.missing.push("owner/config.json".into()),
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&state).unwrap();
        std::fs::write(ctx.state(), &bytes).unwrap();
        assert!(load_state(&ctx).is_err(), "{field}");
        assert_eq!(show(&ctx)["state_available"], false);
        assert_eq!(std::fs::read(ctx.state()).unwrap(), bytes);
    }
}

#[tokio::test]
async fn package_setup_record_and_orphan_lease_quotas_refuse_before_source_io() {
    for orphan in [false, true] {
        let (_root, ctx) = fixture("eefn", "1.0.0");
        configure(&ctx, Mode::Auto, true, None, None).unwrap();
        let (template, serial, op) = admit(&ctx, Phase::Checking);
        drop(op);
        drop(serial);
        let records = if orphan {
            vec![]
        } else {
            (0..RECORD_MAX)
                .map(|_| {
                    let mut r = template.clone();
                    r.id = uuid::Uuid::new_v4().to_string();
                    r.policy_id = uuid::Uuid::new_v4().to_string();
                    r.grant.policy_id = Some(r.policy_id.clone());
                    r.phase = Phase::Failed;
                    r
                })
                .collect()
        };
        std::fs::write(
            ctx.state(),
            serde_json::to_vec(&State {
                schema_version: 1,
                program: ctx.program.clone(),
                records,
            })
            .unwrap(),
        )
        .unwrap();
        if orphan {
            for _ in 1..RECORD_MAX {
                std::fs::write(ctx.operation_lease(&uuid::Uuid::new_v4().to_string()), []).unwrap();
            }
        }
        let before = std::fs::read(ctx.state()).unwrap();
        assert!(
            load_state(&ctx).is_ok(),
            "quota fixture must otherwise be valid"
        );
        let source = FakeSource::new(0);
        assert!(drive(&ctx, None, Control::local(), &source).await.is_err());
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(ctx.state()).unwrap(), before);
        if orphan {
            assert_eq!(show(&ctx)["lease_slots"], RECORD_MAX);
            assert_eq!(show(&ctx)["orphan_lease_count"], RECORD_MAX);
        }
    }
}
