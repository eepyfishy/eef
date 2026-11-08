// Opt-in Windows component acceptance, not a microphone/device test. The exact
// shipping one-shot host invokes a synthetic WAV producer under its real owned
// job and microphone reservation; no sound driver or network is involved.
#[tokio::test]
async fn reserved_one_shot_wav_capture_releases_only_after_owned_exit_and_can_run_again() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tracing::instrument::WithSubscriber;
    // Capture only the shipping bridge's numeric timing event for this future.
    // No global subscriber, child text, paths, IDs or plugin parameters are added.
    let diagnostics = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_env_filter("eefn::python=debug")
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_test_writer()
            .finish(),
    );
    let dir = tempfile::tempdir().unwrap();
    let executable = PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("native-test-fixtures requires explicitly pinned Python; no skipped pass"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let plugin_path = dir.path().join("synthetic_wav.py");
    std::fs::write(
        &plugin_path,
        r#"import base64,io,wave
CAPABILITY='audio.capture'
ACTIONS=['record']
def handle(action,params):
    rate=int(params['sample_rate']); channels=int(params['channels'])
    frames=int(float(params['duration_seconds'])*rate)
    output=io.BytesIO()
    with wave.open(output,'wb') as wav:
        wav.setnchannels(channels); wav.setsampwidth(2); wav.setframerate(rate)
        wav.writeframes(bytes(frames*channels*2))
    return {'mime_type':'audio/wav','sample_rate':rate,'channels':channels,
            'data_base64':base64.b64encode(output.getvalue()).decode('ascii')}
"#,
    )
    .unwrap();
    let runtime = PythonRuntime::discover(Some(executable))
        .unwrap()
        .with_limits(BridgeLimits {
            timeout_seconds: 10,
            max_request_bytes: 65536,
            max_stdout_bytes: 1048576,
            max_stderr_bytes: 1024,
            max_job_memory_mib: None,
        })
        .unwrap();
    let plugin = PythonPlugin {
        capability: "audio.capture".into(),
        actions: vec!["record".into()],
        path: plugin_path,
        params: Value::Null,
    };
    let pool = crate::capture_ownership::Pool::default();
    for iteration in 0..2 {
        let reservation = pool
            .reserve()
            .expect("Previous synthetic capture did not release after observed owned exit");
        assert!(
            pool.reserve().is_err(),
            "Reservation must exclude a competing source"
        );
        let observation_started = Instant::now();
        let value = runtime
            .invoke_with_capture_reservation(
                &plugin,
                "record",
                json!({"duration_seconds":0.5,"sample_rate":16000,"channels":1}),
                23728,
                Some(Instant::now() + Duration::from_secs(6)),
                Some(reservation),
            )
            .with_subscriber(diagnostics.clone())
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "Synthetic reserved one-shot failed: iteration={iteration}, observed_elapsed_ms={}, original_execution_budget_ms=6000, error={error:#}",
                    observation_started.elapsed().as_millis(),
                )
            });
        assert_eq!(value["mime_type"], "audio/wav");
        assert_eq!(value["sample_rate"], 16000);
        assert_eq!(value["channels"], 1);
        let wav = STANDARD
            .decode(value["data_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(wav.len(), 16044);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let released = pool
            .reserve()
            .expect("A successful reserved result must have observed exact owned-job exit");
        drop(released);
    }
}
