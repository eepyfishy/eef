//! Private capture-to-model transport composition, no grant/durable job claim.
use super::*;
use crate::python::{
    BridgeLimits, PythonRuntime, stream::PinnedStreamSource, vision::VisionBounds,
};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn owned_vision_reuses_one_camera_frame_and_waits_for_the_local_model_before_next_read() {
    let trace = tempfile::tempdir().unwrap();
    let root = trace.path().to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner = engine(format!("http://{}", listener.local_addr().unwrap()));
    let binding = owner.bind_local_vision(&selection()).unwrap();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let gate = release.clone();
    let (started, received) = tokio::sync::oneshot::channel();
    let peer = async move {
        let mut started = Some(started);
        for (index, body) in [tags(), answer(), tags()].into_iter().enumerate() {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut first = String::new();
            socket.read_line(&mut first).await.unwrap();
            let mut size = 0usize;
            loop {
                let mut header = String::new();
                assert!(socket.read_line(&mut header).await.unwrap() > 0);
                if header == "\r\n" {
                    break;
                }
                if let Some((key, value)) = header.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        size = value.trim().parse().unwrap()
                    }
                }
            }
            assert!(size <= 65536);
            let mut raw = vec![0; size];
            socket.read_exact(&mut raw).await.unwrap();
            if index == 1 {
                assert!(first.starts_with("POST /api/chat "));
                let request: Value = serde_json::from_slice(&raw).unwrap();
                assert_eq!(
                    request["messages"][0]["images"],
                    json!([image().as_base64()])
                );
                started.take().unwrap().send(()).unwrap();
                gate.acquire().await.unwrap().forget();
            } else {
                assert!(first.starts_with("GET /api/tags "))
            }
            socket
                .get_mut()
                .write_all(response(body).as_bytes())
                .await
                .unwrap();
        }
    };
    let prelude = format!(
        r#"import sys as _s,types as _t,pathlib as _p
_f=_t.ModuleType('vision_fixture')
exec({fake},_f.__dict__)
_root=_p.Path({root})
def _log(kind):
    with (_root/'trace').open('a',encoding='utf8') as out:out.write(kind+'\n')
_camera=_f.Camera([_f.Frame(n) for n in (0,20,20)])
_camera.on_open=lambda:_log('open')
_camera.on_read=lambda:_log('read')
_release=_camera.release
def _close():
    _release()
    _log('close')
_camera.release=_close
_cv=_f.cv2(_camera)
_cv.IMWRITE_JPEG_QUALITY=1
_jpeg=bytes.fromhex('ffd801020304ffd9')
_cv.imencode=lambda *args:(True,_t.SimpleNamespace(nbytes=len(_jpeg),tobytes=lambda:_jpeg))
_s.modules['cv2']=_cv
"#,
        fake = serde_json::to_string(include_str!("../../../python/tests/camera_motion_fakes.py"))
            .unwrap(),
        root = serde_json::to_string(trace.path()).unwrap()
    );
    let executable = std::path::PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("Explicit pinned Python required; no skipped native vision pass"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let python = PythonRuntime::discover(Some(executable))
        .unwrap()
        .with_limits(BridgeLimits {
            timeout_seconds: 15,
            max_stdout_bytes: 65536,
            max_job_memory_mib: Some(128),
            ..Default::default()
        })
        .unwrap()
        .with_stream_test_prelude(prelude);
    let bytes = [
        include_bytes!("../../../python/streams/camera_motion.py").as_slice(),
        b"\n",
        include_bytes!("../../../python/streams/camera_vision.py").as_slice(),
    ]
    .concat();
    let source = PinnedStreamSource::new(&bytes, &hex::encode(Sha256::digest(&bytes))).unwrap();
    let params = json!({"max_frames":3,"interval_ms":100,"max_duration_ms":5000,
        "analysis":{"width":320,"height":240,"max_image_bytes":128,"jpeg_quality":70,
            "max_images":1,"min_interval_ms":0,"trigger":"rising"}});
    let bounds = VisionBounds {
        max_frame_bytes: 2048,
        max_stdout_bytes: 65536,
        max_observations: 3,
        max_source_frames: 3,
        max_image_bytes: 128,
        max_images: 1,
    };
    let pool = crate::capture_ownership::Pool::camera();
    let deadline = Instant::now() + Duration::from_secs(15);
    let answers = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = answers.clone();
    let stream = uuid::Uuid::new_v4().to_string();
    let run = python.observe_reserved_vision(
        &source,
        params,
        &stream,
        bounds,
        deadline,
        pool.reserve().unwrap(),
        |observation| {
            let binding = &binding;
            let seen = seen.clone();
            async move {
                if let Some(frame) = observation.image {
                    let answer = binding
                        .analyze(
                            &frame,
                            "Describe movement",
                            &limits(),
                            &granted(),
                            deadline,
                            || Ok(()),
                        )
                        .await?;
                    seen.lock().unwrap().push(answer);
                }
                Ok(())
            }
        },
    );
    let inspection = async {
        received.await.unwrap();
        assert!(pool.reserve().is_err());
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            std::fs::read_to_string(root.join("trace")).unwrap(),
            "open\nread\nread\n"
        );
        release.add_permits(1);
    };
    let (completed, (), ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(run, peer, inspection)
    })
    .await
    .unwrap();
    let completed = completed.unwrap();
    assert_eq!(completed.summary().stream.observations, 3);
    assert_eq!(completed.summary().images, 1);
    assert_eq!(*answers.lock().unwrap(), ["movement detected"]);
    assert_eq!(
        std::fs::read_to_string(root.join("trace")).unwrap(),
        "open\nread\nread\nread\nclose\n"
    );
    assert!(pool.reserve().is_ok());
}
