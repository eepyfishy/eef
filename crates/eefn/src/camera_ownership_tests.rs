//! Synthetic ownership acceptance; no physical camera or OpenCV is used.
use super::*;

fn make_engine(dir: &std::path::Path, service: Arc<crate::NodeService>) -> NodeEngine {
    NodeEngine::new(false, vec![], dir.into())
        .unwrap()
        .with_resources("node".into(), Default::default())
        .unwrap()
        .with_service(service)
        .with_python_runtime(Arc::new(
            PythonRuntime::discover(Some(dir.join("not-executed-python.exe"))).unwrap(),
        ))
}

#[test]
fn camera_runtime_requires_the_exact_service_pool_and_resource_owner() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "node".into());
    let first = make_engine(dir.path(), service.clone());
    let second = make_engine(dir.path(), service.clone());
    assert!(first.camera_runtime("node", &service.camera_pool).is_ok());
    assert!(second.camera_runtime("node", &service.camera_pool).is_ok());
    assert!(first.camera_runtime("other", &service.camera_pool).is_err());
    assert!(
        first
            .camera_runtime("node", &service.microphone_pool)
            .is_err()
    );
    assert!(
        first
            .camera_runtime("node", &crate::capture_ownership::Pool::camera())
            .is_err()
    );
    assert!(
        first
            .microphone_runtime("node", &service.camera_pool)
            .is_err()
    );
    let owner = service.camera_pool.reserve().unwrap();
    assert!(first.camera_pool.reserve().is_err());
    assert!(second.camera_pool.reserve().is_err());
    assert!(first.microphone_pool.reserve().is_ok());
    drop(owner);
    assert!(second.camera_pool.reserve().is_ok());
}

#[test]
fn replacing_service_rebinds_camera_ownership_without_inheriting_another_nodes_pool() {
    let dir = tempfile::tempdir().unwrap();
    let first = crate::NodeService::new(dir.path().join("first.json"), "node".into());
    let second = crate::NodeService::new(dir.path().join("second.json"), "node".into());
    let engine = make_engine(dir.path(), first.clone()).with_service(second.clone());
    let first_owner = first.camera_pool.reserve().unwrap();
    assert!(engine.camera_runtime("node", &first.camera_pool).is_err());
    assert!(engine.camera_runtime("node", &second.camera_pool).is_ok());
    let second_owner = engine.camera_pool.reserve().unwrap();
    assert!(second.camera_pool.reserve().is_err());
    assert!(first.camera_pool.reserve().is_err());
    drop(second_owner);
    assert!(second.camera_pool.reserve().is_ok());
    assert!(first.camera_pool.reserve().is_err());
    drop(first_owner);
}

#[cfg(windows)]
#[tokio::test]
async fn busy_camera_is_refused_by_actual_one_shot_dispatch_before_plugin_or_interpreter_use() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "node".into());
    let mut engine = make_engine(dir.path(), service.clone());
    engine.policy.media.camera = true;
    engine.policy.media.microphone = true;
    for capability in ["camera.capture", "audio.capture"] {
        engine.plugins.insert(
            capability.into(),
            PythonPlugin {
                capability: capability.into(),
                actions: vec!["capture".into()],
                path: dir.path().join("never-inspected.py"),
                params: Value::Null,
            },
        );
    }
    let camera = service.camera_pool.reserve().unwrap();
    let microphone = service.microphone_pool.reserve().unwrap();
    for (capability, expected) in [
        ("camera.capture", "Camera"),
        ("audio.capture", "Microphone"),
    ] {
        let error = engine
            .execute(capability, "capture", json!({}))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{expected} source is busy or its previous exit is unconfirmed")
        );
    }
    drop(camera);
    assert!(service.camera_pool.reserve().is_ok());
    assert!(service.microphone_pool.reserve().is_err());
    drop(microphone);
}

#[cfg(all(windows, feature = "native-test-fixtures"))]
#[tokio::test]
async fn synthetic_one_shot_camera_dispatch_releases_shared_pool_only_after_owned_exit() {
    use base64::Engine as _;
    let dir = tempfile::tempdir().unwrap();
    let executable = PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("native-test-fixtures requires explicitly pinned Python; no skipped pass"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let plugin_path = dir.path().join("synthetic_camera.py");
    std::fs::write(&plugin_path, r#"import base64,binascii,pathlib,struct,time,zlib
CAPABILITY='camera.capture'
ACTIONS=['capture']
def handle(action,params):
    pathlib.Path(params['started_file']).write_text('ready')
    while not pathlib.Path(params['release_file']).is_file():
        time.sleep(0.01)
    def chunk(kind,data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',binascii.crc32(kind+data)&0xffffffff)
    image=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,8,2,0,0,0))
    image+=chunk(b'IDAT',zlib.compress(b'\x00\x00\x00\x00'))+chunk(b'IEND',b'')
    return {'mime_type':'image/png','data_base64':base64.b64encode(image).decode('ascii')}
"#).unwrap();
    let runtime = PythonRuntime::discover(Some(executable)).unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "node".into());
    let mut engine =
        make_engine(dir.path(), service.clone()).with_python_runtime(Arc::new(runtime));
    engine.policy.media.camera = true;
    engine.plugins.insert(
        "camera.capture".into(),
        PythonPlugin {
            capability: "camera.capture".into(),
            actions: vec!["capture".into()],
            path: plugin_path,
            params: Value::Null,
        },
    );
    // A node-wide conservative camera reservation also covers another engine.
    let observer = make_engine(dir.path(), service.clone());
    for iteration in 0..2 {
        let camera = service.camera_pool.reserve().unwrap();
        assert!(
            engine
                .execute("camera.capture", "capture", json!({}))
                .await
                .unwrap_err()
                .to_string()
                .starts_with("Camera source is busy")
        );
        drop(camera);
        let started_file = dir.path().join(format!("started-{iteration}"));
        let release_file = dir.path().join(format!("release-{iteration}"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        let capture = engine.execute_with_python_output_limit(
            "camera.capture",
            "capture",
            json!({"started_file":started_file,"release_file":release_file}),
            Some(1024),
            Some(deadline),
        );
        tokio::pin!(capture);
        tokio::select! {
            result=&mut capture=>panic!("Synthetic capture completed before release: {result:?}"),
            observed=tokio::time::timeout_at(deadline, async {
                while !started_file.is_file() { tokio::time::sleep(Duration::from_millis(10)).await; }
            })=>observed.unwrap(),
        }
        assert!(observer.camera_pool.reserve().is_err());
        assert!(service.camera_pool.reserve().is_err());
        assert!(service.microphone_pool.reserve().is_ok());
        std::fs::write(release_file, "release").unwrap();
        let value = capture.await.unwrap();
        assert_eq!(value["mime_type"], "image/png");
        let image = base64::engine::general_purpose::STANDARD
            .decode(value["data_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&image[image.len() - 8..image.len() - 4], b"IEND");
        let released = observer
            .camera_pool
            .reserve()
            .expect("Successful capture must observe owned-job exit before reuse");
        assert!(service.camera_pool.reserve().is_err());
        drop(released);
        assert!(service.microphone_pool.reserve().is_ok());
    }
}
