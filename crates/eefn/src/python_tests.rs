//! Deterministic pipe bounds, not device/Python acceptance.
use super::*;

#[tokio::test]
async fn availability_failures_are_typed_without_private_paths_or_errors() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = "PRIVATE-interpreter-path-token";
    let mut runtime = PythonRuntime::discover(Some(dir.path().join(sentinel))).unwrap();
    let failure = runtime.check_availability().await.unwrap_err();
    assert_eq!(failure, AvailabilityFailure::Spawn);
    assert!(!failure.to_string().contains(sentinel));
    let error = runtime.check_available().await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<AvailabilityFailure>(),
        Some(&AvailabilityFailure::Spawn)
    );
    runtime.limits.timeout_seconds = 0;
    assert_eq!(
        runtime.check_availability().await.unwrap_err(),
        AvailabilityFailure::InvalidLimits
    );
}

#[test]
fn startup_observations_are_constant_shape_and_do_not_forward_untyped_errors() {
    let sentinel = "PRIVATE-plugin-path-output-token";
    assert_eq!(
        PluginStartupFailure::from_error(&anyhow::anyhow!(sentinel)),
        PluginStartupFailure::Other {}
    );
    let error = anyhow::Error::from(BridgeFailure::Process).context(sentinel);
    let failure = PluginStartupFailure::from_error(&error);
    assert_eq!(
        failure,
        PluginStartupFailure::Bridge {
            reason: BridgeFailure::Process
        }
    );
    let mut counts = PluginStartupCounts {
        requested: 2,
        ..Default::default()
    };
    counts.record(None);
    counts.record(Some(failure));
    assert_eq!((counts.requested, counts.loaded, counts.failed), (2, 1, 1));
    let report = PythonStartup {
        complete: true,
        interpreter: InterpreterStartup::Available {},
        builtin_plugins: counts.clone(),
        custom_plugins: PluginStartupCounts::default(),
    };
    let value = serde_json::to_value(&report).unwrap();
    assert!(!value.to_string().contains(sentinel));
    assert_eq!(
        serde_json::from_value::<PythonStartup>(value.clone()).unwrap(),
        report
    );
    let mut injected = value.clone();
    injected["interpreter"]["private"] = json!(sentinel);
    assert!(serde_json::from_value::<PythonStartup>(injected).is_err());
    let mut injected = value;
    injected["builtin_plugins"]["last_failure"]["reason"] = json!(sentinel);
    assert!(serde_json::from_value::<PythonStartup>(injected).is_err());
    for state in [
        "not_requested",
        "checking",
        "not_configured",
        "invalid_limits",
        "available",
    ] {
        let mut value = json!({"state": state});
        assert!(serde_json::from_value::<InterpreterStartup>(value.clone()).is_ok());
        value["private"] = json!(sentinel);
        assert!(serde_json::from_value::<InterpreterStartup>(value).is_err());
    }
    for stage in ["lookup", "other"] {
        let mut value = json!({"stage": stage});
        assert!(serde_json::from_value::<PluginStartupFailure>(value.clone()).is_ok());
        value["private"] = json!(sentinel);
        assert!(serde_json::from_value::<PluginStartupFailure>(value).is_err());
    }
    counts.loaded = u32::MAX;
    counts.failed = u32::MAX;
    counts.record(None);
    counts.record(Some(PluginStartupFailure::Lookup {}));
    assert_eq!((counts.loaded, counts.failed), (u32::MAX, u32::MAX));
}

#[test]
fn managed_commands_disable_import_cache_writes_and_keep_isolation_and_utf8() {
    let runtime = PythonRuntime::discover(Some(PathBuf::from("owned-python.exe"))).unwrap();
    let command = runtime.command();
    assert_eq!(command.as_std().get_program(), "owned-python.exe");
    assert_eq!(
        command.as_std().get_args().collect::<Vec<_>>(),
        ["-I", "-B", "-X", "utf8"]
    );
}

#[test]
fn bundled_components_python_override_precedence_and_missing_managed_interpreter_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("versions/0.4.0-test");
    std::fs::create_dir_all(&bundle).unwrap();
    let exe = bundle.join("eefn.exe");
    std::fs::write(&exe, b"fixture").unwrap();
    let layout = crate::runtime_layout::RuntimeLayout::from_executable(&exe).unwrap();
    let explicit = dir.path().join("owner-python.exe");
    let environment = dir.path().join("environment-python.exe");
    assert_eq!(
        PythonRuntime::discover_in(
            Some(explicit.clone()),
            Some(environment.clone()),
            Some(&layout)
        )
        .unwrap()
        .executable(),
        explicit
    );
    assert_eq!(
        PythonRuntime::discover_in(None, Some(environment.clone()), Some(&layout))
            .unwrap()
            .executable(),
        environment
    );
    let managed = PythonRuntime::discover_in(None, None, Some(&layout)).unwrap();
    assert_eq!(managed.executable(), layout.bundled_python().unwrap());
    assert!(!managed.executable().is_file());
    assert!(PythonRuntime::discover_in(None, None, None).is_none());
    assert_eq!(
        PythonRuntime::discover_in(Some(explicit.clone()), None, None)
            .unwrap()
            .executable(),
        explicit
    );
    let flat = dir.path().join("development/eefn.exe");
    std::fs::create_dir_all(flat.parent().unwrap()).unwrap();
    std::fs::write(&flat, b"fixture").unwrap();
    let development = crate::runtime_layout::RuntimeLayout::from_executable(flat).unwrap();
    assert_eq!(
        PythonRuntime::discover_in(None, None, Some(&development))
            .unwrap()
            .executable(),
        Path::new(if cfg!(windows) {
            "python.exe"
        } else {
            "python3"
        })
    );
}
#[tokio::test]
async fn oversized_request_is_refused_before_plugin_metadata_or_interpreter_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PythonRuntime::discover(Some(dir.path().join("does-not-exist.exe")))
        .unwrap()
        .with_limits(BridgeLimits {
            max_request_bytes: 1024,
            ..Default::default()
        })
        .unwrap();
    let error = runtime
        .call(
            &dir.path().join("does-not-exist.py"),
            json!({"op":"invoke","params":{"data":"x".repeat(2048)}}),
            1024,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::RequestLimit)
    );
}
#[test]
fn limits_are_compatible_bounded_and_strict() {
    let defaults: BridgeLimits = serde_json::from_value(json!({})).unwrap();
    assert_eq!(defaults, BridgeLimits::default());
    defaults.validate().unwrap();
    assert!(
        serde_json::to_value(&defaults)
            .unwrap()
            .get("max_job_memory_mib")
            .is_none()
    );
    for value in [
        json!({"timeout_seconds":0}),
        json!({"timeout_seconds":301}),
        json!({"max_request_bytes":1023}),
        json!({"max_stdout_bytes":64*1024*1024+1}),
        json!({"max_stderr_bytes":1024*1024+1}),
        json!({"max_job_memory_mib":63}),
        json!({"max_job_memory_mib":1048577}),
    ] {
        assert!(
            serde_json::from_value::<BridgeLimits>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    assert!(serde_json::from_value::<BridgeLimits>(json!({"unbounded":true})).is_err());
}
#[test]
fn request_encoding_refuses_overflow_before_start_and_reply_errors_are_private() {
    let request = json!({"private":"PRIVATE_MICROPHONE_PAYLOAD"});
    let expected = serde_json::to_vec(&request).unwrap();
    assert_eq!(encode_request(&request, expected.len()).unwrap(), expected);
    let error = encode_request(&request, expected.len() - 1).unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::RequestLimit)
    );
    assert!(!format!("{error:#}").contains("PRIVATE_MICROPHONE_PAYLOAD"));
    for invalid in [
        b"PRIVATE_MICROPHONE_PAYLOAD".as_slice(),
        b"{\"ok\":false,\"error\":\"PRIVATE_MICROPHONE_PAYLOAD\"}",
        b"[]",
    ] {
        let error = decode_reply(invalid).unwrap_err();
        assert_eq!(
            error.downcast_ref::<BridgeFailure>(),
            Some(&BridgeFailure::Reply)
        );
        assert!(!format!("{error:#}").contains("PRIVATE_MICROPHONE_PAYLOAD"));
    }
    assert_eq!(
        decode_reply(br#"{"ok":true,"data":{"bytes":9}}"#).unwrap(),
        json!({"bytes":9})
    );
}
#[tokio::test]
async fn exact_stdout_and_stderr_bounds_never_parse_a_truncated_prefix() {
    let data = vec![b'a'; 1024];
    assert_eq!(
        read_pipe(data.as_slice(), 1024, BridgeFailure::StdoutLimit)
            .await
            .unwrap(),
        data
    );
    for failure in [BridgeFailure::StdoutLimit, BridgeFailure::StderrLimit] {
        let error = read_pipe(data.as_slice(), 1023, failure).await.unwrap_err();
        assert_eq!(error.downcast_ref::<BridgeFailure>(), Some(&failure));
    }
}
#[tokio::test]
async fn full_exchange_drains_outputs_while_stdin_is_backpressured() {
    let (stdin, mut host_input) = tokio::io::duplex(32);
    let (mut host_output, stdout) = tokio::io::duplex(32);
    let (mut host_error, stderr) = tokio::io::duplex(32);
    let input = vec![b'r'; 2048];
    let fixture = async {
        host_output.write_all(&vec![b'o'; 1024]).await.unwrap();
        host_error.write_all(&vec![b'e'; 1024]).await.unwrap();
        drop(host_output);
        drop(host_error);
        let mut received = Vec::new();
        host_input.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, input);
    };
    let call = exchange(
        stdin,
        stdout,
        stderr,
        &input,
        1024,
        1024,
        Instant::now() + Duration::from_secs(2),
        async { Ok(7) },
    );
    let (_, result) = tokio::join!(fixture, call);
    let (status, output) = result.unwrap();
    assert_eq!(status, 7);
    assert_eq!(output, vec![b'o'; 1024]);
}
#[tokio::test]
async fn full_exchange_deadline_includes_blocked_stdin_and_inherited_output() {
    let (stdin, _unread_input) = tokio::io::duplex(1);
    let (held_stdout, stdout) = tokio::io::duplex(1);
    let (held_stderr, stderr) = tokio::io::duplex(1);
    let started = Instant::now();
    let error = exchange(
        stdin,
        stdout,
        stderr,
        &[b'r'; 2048],
        1024,
        1024,
        started + Duration::from_millis(50),
        async { Ok(()) },
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::Deadline)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    drop((held_stdout, held_stderr));
}
#[tokio::test]
async fn stderr_flood_fails_before_a_blocked_request_can_finish() {
    let (stdin, _unread_input) = tokio::io::duplex(1);
    let (held_stdout, stdout) = tokio::io::duplex(1);
    let error = exchange(
        stdin,
        stdout,
        &[b'e'; 1025][..],
        &[b'r'; 2048],
        1024,
        1024,
        Instant::now() + Duration::from_secs(2),
        async { Ok(()) },
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::StderrLimit)
    );
    drop(held_stdout);
}
