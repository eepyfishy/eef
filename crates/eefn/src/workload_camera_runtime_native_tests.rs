// Included only by Windows native-test-fixtures; no shipping hook or hardware.
use futures_util::FutureExt;
include!("workload_camera_opencv_native_tests.rs");
pub(crate) fn native_fixture(mode: &str) -> (Fixture, tempfile::TempDir) {
    let trace = tempfile::tempdir().unwrap();
    let executable = std::path::PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("Explicit pinned Python required; no skipped native pass"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let prelude = format!(
        "_eef_camera_root = {}\n_eef_camera_mode = {}\n_eef_camera_fake_source = {}\n{}",
        serde_json::to_string(trace.path()).unwrap(),
        serde_json::to_string(mode).unwrap(),
        serde_json::to_string(include_str!("../../../python/tests/camera_motion_fakes.py"))
            .unwrap(),
        include_str!("../../../python/tests/prepared_camera_device.py")
    );
    let python = PythonRuntime::discover(Some(executable))
        .unwrap()
        .with_stream_test_prelude(prelude);
    (Fixture::with_python(None, Some(python), true), trace)
}
fn trace_kinds(root: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(root.join("trace.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}
async fn blocked(root: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(root.join("trace.jsonl")).is_ok_and(|raw| {
                raw.lines().any(|line| {
                    serde_json::from_str::<Value>(line).is_ok_and(|v| v["kind"] == "blocked")
                })
            }) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
async fn reusable(f: &Fixture, processes: &crate::process_lifetime::tests::Processes) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(next) = f.service.camera_pool.reserve() {
                assert!(
                    processes.exited_now(),
                    "Root and descendant must signal before camera reuse"
                );
                drop(next);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_camera_runner_uses_pinned_source_durable_events_and_owned_exit_before_reuse() {
    let (f, trace) = native_fixture("finite");
    let source = f.prepare().unwrap();
    assert!(!trace.path().join("pids.json").exists());
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let deadline = tokio::time::Instant::from_std(consumed.session_deadline().unwrap());
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let root = trace.path().to_owned();
    let saved = ledger.clone();
    let definition = f.definition.clone();
    let run = source.run(consumed, renewals, move |event| {
        let seen = seen.clone();
        let root = root.clone();
        let saved = saved.clone();
        let definition = definition.clone();
        async move {
            let durable = saved.workload_events(
                &definition.issuer_id,
                &definition.grant_id,
                event.sequence - 1,
                1,
            )?;
            ensure!(
                durable["durable_events"][0]["event"] == json!(event),
                "Camera callback preceded durable event persistence"
            );
            let sequence = event.source_sequence;
            seen.lock().unwrap().push(event);
            tokio::fs::write(root.join(format!("ack-{sequence}")), b"observed").await?;
            Ok(())
        }
    });
    assert!(std::mem::size_of_val(&run) <= 64 * 1024);
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe_until(trace.path(),deadline)=>processes,
        result=&mut run=>panic!("Camera ended before owned process observation: {result:?}"),
    };
    processes.alive();
    assert!(f.service.camera_pool.reserve().is_err());
    assert!(f.service.microphone_pool.reserve().is_ok());
    tokio::fs::write(trace.path().join("observed"), b"held handles")
        .await
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(15), &mut run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.receipt.state, "completed");
    assert_eq!(completed.definition, f.definition);
    let completion = completed.completion.as_ref().unwrap();
    assert_eq!(
        completion.adapter_sha256,
        crate::workload_sources::camera_adapter_digest()
    );
    assert_eq!(
        (
            completion.source_observations,
            completion.source_frames,
            completion.event_count
        ),
        (5, 5, 5)
    );
    assert!(completion.owned_process_exit_observed && !completion.physical_effects_stopped);
    assert!(processes.exited_now());
    assert!(f.service.camera_pool.reserve().is_ok());
    assert_eq!(trace_kinds(trace.path()), ["opened", "closed"]);
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| (
                e.source_sequence,
                e.source_frames,
                e.active,
                e.discontinuity
            ))
            .collect::<Vec<_>>(),
        [
            (1, 1, None, false),
            (2, 2, Some(true), false),
            (3, 3, Some(false), false),
            (4, 4, None, true),
            (5, 5, Some(false), false)
        ]
    );
    assert!(
        events
            .iter()
            .all(|e| e.kind == crate::workload_events::Kind::CameraMotion)
    );
    let encoded = serde_json::to_string(&*events).unwrap();
    for private in ["PRIVATE_", "rtsp://", "data_base64", "pixels", "image"] {
        assert!(!encoded.contains(private));
    }
    let reopened = crate::grants::GrantLedger::open(
        &f.dir.path().join("receipts.db"),
        Default::default(),
        "node",
    )
    .unwrap();
    assert_eq!(
        json!(
            reopened
                .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
                .unwrap()
        ),
        json!(completed)
    );
}

#[tokio::test]
async fn native_camera_permission_revocation_or_scoped_stop_retires_owned_source_without_restarting()
 {
    for stop in [false, true] {
        let (f, trace) = native_fixture("blocked");
        let source = f.prepare().unwrap();
        let (ledger, consumed) = consume(&f, f.definition.clone()).await;
        let deadline = tokio::time::Instant::from_std(consumed.session_deadline().unwrap());
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        let run = source.run(consumed, renewals, |_| async {
            panic!("Blocked camera emitted an observation");
            #[allow(unreachable_code)]
            Ok(())
        });
        tokio::pin!(run);
        let processes = tokio::select! {
            processes=crate::process_lifetime::tests::Processes::observe_until(trace.path(),deadline)=>processes,
            result=&mut run=>panic!("Camera ended before owned process observation: {result:?}"),
        };
        tokio::fs::write(trace.path().join("observed"), b"held handles")
            .await
            .unwrap();
        tokio::select! {_=blocked(trace.path())=>{},result=&mut run=>panic!("Camera ended before revocation: {result:?}")};
        processes.alive();
        assert!(f.service.camera_pool.reserve().is_err());
        assert!(f.service.microphone_pool.reserve().is_ok());
        if stop {
            ledger
                .request_stop_scope(
                    &f.definition.scope().unwrap(),
                    &f.definition.issuer_id,
                    &f.definition.session_id,
                    "node",
                )
                .unwrap();
        } else {
            f.save(|c| c["permissions"]["media"]["camera"] = json!(false));
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut run)
                .await
                .unwrap()
                .is_err()
        );
        reusable(&f, &processes).await;
        unknown(&f, &ledger, &f.definition).await;
        assert_eq!(
            trace_kinds(trace.path())
                .iter()
                .filter(|kind| kind.as_str() == "opened")
                .count(),
            1
        );
        assert!(f.service.restart.notified().now_or_never().is_none());
    }
}

#[tokio::test]
async fn native_camera_whole_future_drop_observes_root_and_descendant_before_reservation_reuse() {
    let (f, trace) = native_fixture("blocked");
    let source = f.prepare().unwrap();
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let deadline = tokio::time::Instant::from_std(consumed.session_deadline().unwrap());
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let processes;
    {
        let run = source.run(consumed, renewals, |_| async {
            panic!("Blocked camera emitted an observation");
            #[allow(unreachable_code)]
            Ok(())
        });
        tokio::pin!(run);
        processes = tokio::select! {
            processes=crate::process_lifetime::tests::Processes::observe_until(trace.path(),deadline)=>processes,
            result=&mut run=>panic!("Camera ended before owned process observation: {result:?}"),
        };
        tokio::fs::write(trace.path().join("observed"), b"held handles")
            .await
            .unwrap();
        tokio::select! {_=blocked(trace.path())=>{},result=&mut run=>panic!("Camera ended before whole-task cancellation: {result:?}")};
        processes.alive();
        assert!(f.service.camera_pool.reserve().is_err());
    }
    reusable(&f, &processes).await;
    unknown(&f, &ledger, &f.definition).await;
}
