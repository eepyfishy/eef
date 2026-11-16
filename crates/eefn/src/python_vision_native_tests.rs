//! Owned native Python processes and synthetic private frames; no device/model.
use super::*;
use std::sync::Arc;

fn runtime() -> PythonRuntime {
    let executable = std::path::PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("Native vision requires the explicitly pinned interpreter; no skipped pass"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    PythonRuntime::discover(Some(executable))
        .unwrap()
        .with_limits(BridgeLimits {
            timeout_seconds: 15,
            max_job_memory_mib: Some(128),
            ..Default::default()
        })
        .unwrap()
}
fn source() -> PinnedStreamSource {
    let bytes=br#"import base64,json,os,pathlib,subprocess,sys
def stream(action,params):
    if action!='vision':raise ValueError('synthetic action')
    root=pathlib.Path(params['trace_root'])
    child=subprocess.Popen([sys.executable,'-I','-B','-c','import time; time.sleep(30)'],
        stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=0x08000000)
    (root/'pids.json').write_text(json.dumps({'root':os.getpid(),'child':child.pid}),encoding='utf8')
    try:
        yield {'source_frames':1,'active':None,'level':None,'discontinuity':False,'analysis_state':'not_requested','image_base64':None}
        image=base64.b64encode(bytes.fromhex('ffd801020304ffd9')).decode('ascii')
        yield {'source_frames':2,'active':True,'level':20,'discontinuity':False,'analysis_state':'frame','image_base64':image}
    finally:
        (root/'closed').write_text('closed',encoding='utf8')
"#;
    PinnedStreamSource::new(bytes, &hex::encode(Sha256::digest(bytes))).unwrap()
}
fn params(root: &std::path::Path) -> Value {
    json!({"max_frames":2,"analysis":{"max_image_bytes":128,"max_images":1},"trace_root":root})
}

#[tokio::test]
async fn owned_private_vision_paces_consumer_and_witnesses_root_and_descendant_before_reuse() {
    let runtime = runtime();
    let source = source();
    let id = id();
    let root = tempfile::tempdir().unwrap();
    let pool = crate::capture_ownership::Pool::camera();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let release = gate.clone();
    let deadline = Instant::now() + Duration::from_secs(15);
    let run = runtime.observe_reserved_vision(
        &source,
        params(root.path()),
        &id,
        bounds(),
        deadline,
        pool.reserve().unwrap(),
        move |observation| {
            let gate = release.clone();
            async move {
                gate.acquire().await.unwrap().forget();
                if observation.motion.sequence == 1 {
                    assert!(observation.image.is_none());
                } else {
                    assert_eq!(observation.analysis, AnalysisState::Frame);
                    assert_eq!(observation.image.unwrap().bytes(), 8);
                }
                Ok(())
            }
        },
    );
    assert!(std::mem::size_of_val(&run) <= 64 * 1024);
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe_until(root.path(),deadline)=>processes,
        result=&mut run=>panic!("Vision returned before process observation: {result:?}"),
    };
    processes.alive();
    assert!(pool.reserve().is_err());
    gate.add_permits(2);
    let observed = run.await.unwrap();
    assert_eq!(observed.source_digest(), source.digest());
    assert_eq!(observed.summary().stream.observations, 2);
    assert_eq!(observed.summary().images, 1);
    assert!(root.path().join("closed").is_file());
    assert!(processes.exited_now());
    assert!(pool.reserve().is_ok());
}

#[tokio::test]
async fn failed_private_consumer_retires_exact_process_tree_without_an_exit_or_success_report() {
    let runtime = runtime();
    let source = source();
    let id = id();
    let root = tempfile::tempdir().unwrap();
    let pool = crate::capture_ownership::Pool::camera();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let release = gate.clone();
    let deadline = Instant::now() + Duration::from_secs(15);
    let run = runtime.observe_reserved_vision(
        &source,
        params(root.path()),
        &id,
        bounds(),
        deadline,
        pool.reserve().unwrap(),
        move |_| {
            let gate = release.clone();
            async move {
                gate.acquire().await.unwrap().forget();
                bail!("PRIVATE-vision-consumer-error")
            }
        },
    );
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe_until(root.path(),deadline)=>processes,
        result=&mut run=>panic!("Vision returned before process observation: {result:?}"),
    };
    processes.alive();
    assert!(pool.reserve().is_err());
    gate.add_permits(1);
    let error = run.await.unwrap_err();
    assert!(!error.to_string().contains("PRIVATE"));
    assert!(processes.exited_now());
    assert!(pool.reserve().is_ok());
}

#[tokio::test]
async fn private_vision_original_deadline_retires_a_held_consumer_and_its_process_tree() {
    let runtime = runtime();
    let source = source();
    let id = id();
    let root = tempfile::tempdir().unwrap();
    let pool = crate::capture_ownership::Pool::camera();
    // Start-up and handle observation get the ordinary native-test allowance.
    // The held consumer then consumes the remaining original deadline; there
    // is no replacement lease or per-frame extension after observation.
    let deadline = Instant::now() + Duration::from_secs(8);
    let run = runtime.observe_reserved_vision(
        &source,
        params(root.path()),
        &id,
        bounds(),
        deadline,
        pool.reserve().unwrap(),
        |_| async { std::future::pending::<Result<()>>().await },
    );
    tokio::pin!(run);
    let processes = tokio::select! {
        processes=crate::process_lifetime::tests::Processes::observe_until(root.path(),deadline)=>processes,
        result=&mut run=>panic!("Vision returned before process observation: {result:?}"),
    };
    processes.alive();
    assert!(pool.reserve().is_err());
    let error = run.await.unwrap_err();
    assert!(error.is::<BridgeFailure>());
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::Deadline)
    );
    assert!(processes.exited_now());
    assert!(pool.reserve().is_ok());
}
