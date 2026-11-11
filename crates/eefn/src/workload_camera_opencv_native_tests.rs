// Included exclusively by the opt-in Windows native camera fixture module.
// Real locked OpenCV/NumPy and encoded images; acquisition is a generated file,
// not a physical webcam, RTSP transport, model or shipping file-source feature.
#[tokio::test]
async fn native_camera_real_decoder_persists_motion_and_keeps_truncated_input_unknown() {
    real_decoder_cases(&[(5, 512, 160, 120), (3, 512, 160, 120)]).await;
}

#[tokio::test]
async fn native_camera_real_decoder_honors_default_command_memory_and_resolution() {
    #[derive(clap::Parser)]
    struct DefaultCamera {
        #[command(flatten)]
        camera: Box<crate::camera_submission::CameraSubmission>,
    }
    let defaults = <DefaultCamera as clap::Parser>::try_parse_from(["camera-default-fixture"])
        .unwrap()
        .camera;
    assert_eq!(defaults.limits.max_job_memory_mib, 128);
    assert_eq!((defaults.capture.width, defaults.capture.height), (640, 480));
    // Five finite images exercise decoding under the actual CLI's memory and
    // resolution defaults. This is not physical capture or a full setup test.
    real_decoder_cases(&[(
        5,
        defaults.limits.max_job_memory_mib,
        defaults.capture.width,
        defaults.capture.height,
    )])
    .await;
}

async fn real_decoder_cases(cases: &[(usize, u64, u32, u32)]) {
    for &(frames, memory_mib, width, height) in cases {
        let trace = tempfile::tempdir().unwrap();
        let executable = std::path::PathBuf::from(
            std::env::var_os("EEF_TEST_CAMERA_PYTHON")
                .expect("Explicit locked OpenCV/NumPy interpreter required"),
        );
        assert!(executable.is_absolute() && executable.is_file());
        let prelude = format!(
            "_eef_decode_root = {}\n_eef_decode_frames = {}\n_eef_decode_size = ({}, {})\n{}",
            serde_json::to_string(trace.path()).unwrap(),
            frames,
            width,
            height,
            include_str!("../../../python/tests/prepared_camera_opencv.py")
        );
        let python = PythonRuntime::discover(Some(executable))
            .unwrap()
            .with_stream_test_prelude(prelude);
        let mut f = Fixture::with_python(None, Some(python), true);
        // Each case has its own reviewed budget; never widen a granted limit.
        f.definition.limits.max_job_memory_mib = memory_mib;
        let Source::CameraMotion { parameters, .. } = &f.definition.source else {
            panic!("Camera decoder fixture lost its reviewed camera source");
        };
        assert_eq!((parameters.width, parameters.height), (640, 480));
        assert!(width <= parameters.width && height <= parameters.height);
        f.definition.validate().unwrap();
        let source = f.prepare().unwrap();
        assert_eq!(source.python.limits().max_job_memory_mib, Some(memory_mib));
        assert!(!trace.path().join("generated.avi").exists());
        assert!(f.service.camera_pool.reserve().is_err());
        let (ledger, consumed) = consume(&f, f.definition.clone()).await;
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = events.clone();
        let saved = ledger.clone();
        let definition = f.definition.clone();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            source.run(consumed, renewals, move |event| {
                let observed = observed.clone();
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
                        "Decoded motion callback preceded durable persistence"
                    );
                    observed.lock().unwrap().push(event);
                    Ok(())
                }
            }),
        )
        .await
        .unwrap();
        if frames == 5 {
            assert!(
                outcome.is_ok(),
                "Real decoder workload failed at {width}x{height} / {memory_mib} MiB: {outcome:?}; fixture stages: {:?}",
                if trace.path().join("trace.jsonl").is_file() {
                    trace_kinds(trace.path())
                } else {
                    Vec::new()
                }
            );
            let completed = outcome.unwrap();
            assert_eq!(completed.receipt.state, "completed");
            assert_eq!(completed.definition, f.definition);
            let completion = completed.completion.unwrap();
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
            assert!(completion.owned_process_exit_observed);
            assert!(!completion.physical_effects_stopped);
        } else {
            assert!(
                outcome.is_err(),
                "Truncated input cannot complete the reviewed five-frame source"
            );
            unknown(&f, &ledger, &f.definition).await;
            assert!(
                ledger
                    .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
                    .unwrap()
                    .completion
                    .is_none()
            );
        }
        let events = events.lock().unwrap();
        assert_eq!(events.len(), frames);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.source_sequence, index as u64 + 1);
            assert_eq!(event.source_frames, index as u64 + 1);
            assert_eq!(
                event.active,
                [None, Some(true), Some(false), Some(true), Some(false)][index]
            );
            assert!(!event.discontinuity);
            assert_eq!(event.kind, crate::workload_events::Kind::CameraMotion);
        }
        let encoded = serde_json::to_string(&*events).unwrap();
        for private in [
            "PRIVATE_",
            "rtsp://",
            "generated.avi",
            "data_base64",
            "pixels",
            "image",
        ] {
            assert!(!encoded.contains(private));
        }
        drop(events);
        let kinds = trace_kinds(trace.path());
        assert_eq!(kinds.iter().filter(|k| *k == "encoded").count(), 1);
        assert_eq!(
            kinds
                .iter()
                .filter(|k| *k == "unsupported_audio_rejected")
                .count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| *k == "parameters_validated")
                .count(),
            1
        );
        assert_eq!(kinds.iter().filter(|k| *k == "opened").count(), 1);
        assert_eq!(kinds.iter().filter(|k| *k == "closed").count(), 1);
        assert_eq!(kinds.iter().filter(|k| *k == "read").count(), frames);
        assert_eq!(
            kinds.iter().filter(|k| *k == "eof").count(),
            usize::from(frames == 3)
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(reservation) = f.service.camera_pool.reserve() {
                    drop(reservation);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(f.service.restart.notified().now_or_never().is_none());
    }
}
