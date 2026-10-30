// Included only by the opt-in Windows unit-test module, never a shipping hook.
fn native_fixture(mode: &str) -> (Fixture, tempfile::TempDir) {
    let trace = tempfile::tempdir().unwrap();
    let executable = std::path::PathBuf::from(std::env::var_os("EEF_TEST_PYTHON").expect(
        "native-test-fixtures requires the explicitly pinned test Python; no skipped pass",
    ));
    assert!(executable.is_absolute() && executable.is_file());
    let prelude = format!(
        "_eef_fixture_root = {}\n_eef_fixture_mode = {}\n{}",
        serde_json::to_string(trace.path()).unwrap(),
        serde_json::to_string(mode).unwrap(),
        include_str!("../../../python/tests/prepared_source_device.py")
    );
    let python = PythonRuntime::discover(Some(executable))
        .unwrap()
        .with_stream_test_prelude(prelude);
    let mut f = Fixture::with_python(None, Some(python));
    let Source::MicrophoneEnergy { parameters, .. } = &mut f.definition.source;
    parameters.max_blocks = 5;
    f.definition.limits.max_source_frames = 400;
    f.definition.validate().unwrap();
    (f, trace)
}
fn native_trace(root: &std::path::Path) -> Vec<String> {
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
async fn native_wait_blocked(root: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            // The producer may still be creating/appending the trace. Only a
            // complete parsed marker releases this wait; final reads are strict.
            if std::fs::read_to_string(root.join("trace.jsonl")).is_ok_and(|raw| {
                raw.lines().any(|line| {
                    serde_json::from_str::<Value>(line)
                        .is_ok_and(|value| value["kind"] == "blocked")
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
async fn native_wait_reusable(f: &Fixture, processes: &crate::process_lifetime::tests::Processes) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(next) = f.service.microphone_pool.reserve() {
                assert!(
                    processes.exited_now(),
                    "Both held process handles must signal before microphone reuse"
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
fn native_unknown(f: &Fixture, ledger: &crate::grants::GrantLedger) {
    let receipt = ledger
        .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
        .unwrap();
    assert_eq!(receipt.definition, f.definition);
    assert_eq!(receipt.receipt.state, "unknown");
    assert!(!receipt.execution_authorized && !receipt.restart_authorized);
}

#[tokio::test]
async fn native_prepared_source_runs_exact_bytes_with_consumption_and_filtered_events() {
    let (f, trace) = native_fixture("finite");
    let source = f.prepare().unwrap();
    assert_eq!(
        source.source.digest(),
        crate::workload_sources::microphone_adapter_digest()
    );
    assert_eq!(source.params["device"], "PRIVATE_DEVICE");
    assert!(
        !trace.path().join("pids.json").exists(),
        "Preparation must not open a source"
    );
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let ack = consumed.acknowledgement().unwrap();
    assert_eq!(ack.receipt.state, "accepted");
    assert!(
        !trace.path().join("pids.json").exists(),
        "Consumption must not open a source"
    );
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let root = trace.path().to_owned();
    let observed_ledger = ledger.clone();
    let definition = f.definition.clone();
    let run = source.run(consumed, renewals, move |event| {
        let events = seen.clone();
        let root = root.clone();
        let ledger = observed_ledger.clone();
        let definition = definition.clone();
        async move {
            let receipt = ledger.workload_receipt(&definition.issuer_id, &definition.grant_id)?;
            anyhow::ensure!(
                receipt.receipt.state == "accepted" && receipt.definition == definition,
                "Events require the exact consumed source"
            );
            let sequence = event.source_sequence;
            events.lock().unwrap().push(event);
            tokio::fs::write(root.join(format!("ack-{sequence}")), b"observed").await?;
            Ok(())
        }
    });
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe(trace.path())=>processes,
        result=&mut run=>panic!("Source ended before real process observation: {result:?}"),
    };
    processes.alive();
    assert!(f.service.microphone_pool.reserve().is_err());
    tokio::fs::write(trace.path().join("observed"), b"held handles")
        .await
        .unwrap();
    let summary = tokio::time::timeout(std::time::Duration::from_secs(15), &mut run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.stream_id, f.definition.grant_id);
    assert_eq!((summary.observations, summary.source_frames), (5, 400));
    assert!(
        processes.exited_now(),
        "Normal return requires owned root and descendant exit"
    );
    assert!(f.service.microphone_pool.reserve().is_ok());
    assert_eq!(native_trace(trace.path()), ["opened", "closed"]);
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| (
                e.sequence,
                e.source_sequence,
                e.source_frames,
                e.active,
                e.discontinuity
            ))
            .collect::<Vec<_>>(),
        [
            (1, 1, 80, Some(false), false),
            (2, 3, 240, Some(true), false),
            (3, 4, 320, None, true),
            (4, 5, 400, Some(false), false)
        ]
    );
    let encoded = serde_json::to_string(&*events).unwrap();
    for private in [
        "PRIVATE_DEVICE",
        "PRIVATE_CREDENTIAL",
        "data_base64",
        "samples",
        "transcript",
    ] {
        assert!(!encoded.contains(private));
    }
    native_unknown(&f, &ledger);
}

#[tokio::test]
async fn native_prepared_consent_revocation_retires_actual_root_and_descendant() {
    let (f, trace) = native_fixture("blocked");
    let source = f.prepare().unwrap();
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let run = source.run(consumed, renewals, |_| async {
        panic!("Blocked device emitted no observation");
        #[allow(unreachable_code)]
        Ok(())
    });
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe(trace.path())=>processes,
        result=&mut run=>panic!("Source ended before real process observation: {result:?}"),
    };
    processes.alive();
    tokio::fs::write(trace.path().join("observed"), b"held handles")
        .await
        .unwrap();
    tokio::select! {
        _=native_wait_blocked(trace.path())=>{},
        result=&mut run=>panic!("Source ended before consent revocation: {result:?}"),
    }
    assert!(f.service.microphone_pool.reserve().is_err());
    f.listening(false);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), &mut run)
            .await
            .unwrap()
            .is_err()
    );
    native_wait_reusable(&f, &processes).await;
    native_unknown(&f, &ledger);
    assert_eq!(
        native_trace(trace.path())
            .iter()
            .filter(|kind| kind.as_str() == "opened")
            .count(),
        1
    );
    assert!(f.prepare().is_err(), "Revocation must remain effective");
}

#[tokio::test]
async fn native_prepared_whole_future_drop_retires_actual_owned_source() {
    let (f, trace) = native_fixture("blocked");
    let source = f.prepare().unwrap();
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let processes;
    {
        let run = source.run(consumed, renewals, |_| async {
            panic!("Blocked device emitted no observation");
            #[allow(unreachable_code)]
            Ok(())
        });
        tokio::pin!(run);
        processes = tokio::select! {
            processes=crate::process_lifetime::tests::Processes::observe(trace.path())=>processes,
            result=&mut run=>panic!("Source ended before real process observation: {result:?}"),
        };
        processes.alive();
        tokio::fs::write(trace.path().join("observed"), b"held handles")
            .await
            .unwrap();
        tokio::select! {
            _=native_wait_blocked(trace.path())=>{},
            result=&mut run=>panic!("Source ended before whole-task cancellation: {result:?}"),
        }
        assert!(f.service.microphone_pool.reserve().is_err());
        // Drop the actual owned run future here, not just a Pin reference.
    }
    native_wait_reusable(&f, &processes).await;
    native_unknown(&f, &ledger);
    assert_eq!(
        native_trace(trace.path())
            .iter()
            .filter(|kind| kind.as_str() == "opened")
            .count(),
        1
    );
}
