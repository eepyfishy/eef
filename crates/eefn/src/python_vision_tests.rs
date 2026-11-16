use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn bounds() -> VisionBounds {
    VisionBounds {
        max_frame_bytes: 2048,
        max_stdout_bytes: 65536,
        max_observations: 2,
        max_source_frames: 2,
        max_image_bytes: 128,
        max_images: 1,
    }
}
fn ready(id: &str) -> Value {
    json!({"type":"ready","schema_version":1,"protocol":"camera_vision_v1","stream_id":id})
}
fn observation(id: &str, sequence: u64, image: bool) -> Value {
    json!({"type":"observation","stream_id":id,"sequence":sequence,"source_frames":sequence,
        "active":if sequence==1 {Value::Null}else{json!(true)},"level":if sequence==1{Value::Null}else{json!(20)},
        "discontinuity":false,"analysis_state":if image{"frame"}else{"not_requested"},
        "image_base64":if image{json!(jpeg())}else{Value::Null}})
}
fn jpeg() -> String {
    STANDARD.encode([0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9])
}
fn end(id: &str) -> Value {
    json!({"type":"end","stream_id":id,"observations":2,"source_frames":2,"images":1})
}
fn accept(state: &mut wire::State<'_>, value: Value) -> Result<Option<VisionObservation>> {
    state.accept_json(&serde_json::to_vec(&value).unwrap())
}

#[test]
fn vision_bounds_reject_widening_and_mismatched_source_limits() {
    let local = BridgeLimits::default();
    bounds().validate(&local).unwrap();
    for field in [
        "max_frame_bytes",
        "max_stdout_bytes",
        "max_observations",
        "max_source_frames",
        "max_image_bytes",
        "max_images",
    ] {
        let mut raw = json!(bounds());
        raw[field] = json!(u64::MAX);
        if let Ok(widened) = serde_json::from_value::<VisionBounds>(raw) {
            assert!(
                widened.validate(&local).is_err(),
                "Widened {field} accepted"
            );
        }
    }
    let mut limited = local;
    limited.max_stdout_bytes = 1024;
    assert!(bounds().validate(&limited).is_err());
    let params = json!({"max_frames":2,"analysis":{"max_image_bytes":128,"max_images":1}});
    assert_eq!(bounds().expected_frames(&params).unwrap(), 2);
    for changed in [
        json!({"max_frames":0,"analysis":{"max_image_bytes":128,"max_images":1}}),
        json!({"max_frames":2,"analysis":{"max_image_bytes":129,"max_images":1}}),
        json!({"max_frames":2,"analysis":{"max_image_bytes":128,"max_images":true}}),
    ] {
        assert!(bounds().expected_frames(&changed).is_err());
    }
}

#[test]
fn private_jpeg_rejects_invalid_or_oversize_input_and_redacts_debug() {
    let encoded = jpeg();
    let value = PrivateJpeg::checked(encoded.clone(), 128).unwrap();
    assert_eq!(value.as_base64(), encoded);
    assert_eq!(value.bytes(), 8);
    assert_eq!(value.digest().len(), 64);
    assert!(!format!("{value:?}").contains(&encoded));
    for invalid in [
        "PRIVATE-image-payload".into(),
        STANDARD.encode(b"not JPEG"),
        STANDARD.encode([0xff, 0xd8, 0xff, 0xd9].repeat(33)),
    ] {
        let error = PrivateJpeg::checked(invalid, 128).unwrap_err();
        assert!(!error.to_string().contains("PRIVATE"));
    }
}

#[test]
fn private_wire_requires_all_fields_and_rejects_unknown_or_duplicate_data() {
    let id = id();
    let limits = bounds();
    for field in ["active", "level", "image_base64"] {
        let mut state = wire::State::new(&id, &limits, 2);
        accept(&mut state, ready(&id)).unwrap();
        let mut value = observation(&id, 1, false);
        value.as_object_mut().unwrap().remove(field);
        assert!(
            accept(&mut state, value).is_err(),
            "Missing {field} accepted"
        );
    }
    let mut state = wire::State::new(&id, &limits, 2);
    let mut value = ready(&id);
    value["unexpected"] = json!(true);
    assert!(accept(&mut state, value).is_err());
    let duplicate = format!(
        "{{\"type\":\"ready\",\"schema_version\":1,\"schema_version\":1,\"protocol\":\"camera_vision_v1\",\"stream_id\":\"{id}\"}}"
    );
    assert!(state.accept_json(duplicate.as_bytes()).is_err());
    assert!(accept(&mut state, observation(&id, 1, false)).is_err());
}

#[test]
fn private_wire_enforces_scope_positions_image_count_and_complete_end() {
    let id = id();
    let limits = bounds();
    let mut state = wire::State::new(&id, &limits, 2);
    accept(&mut state, ready(&id)).unwrap();
    assert!(accept(&mut state, end(&id)).is_err());
    let mut unsolicited = observation(&id, 1, false);
    unsolicited["image_base64"] = json!(jpeg());
    assert!(accept(&mut state, unsolicited).is_err());
    accept(&mut state, observation(&id, 1, false)).unwrap();
    let record = accept(&mut state, observation(&id, 2, true))
        .unwrap()
        .unwrap();
    assert_eq!(record.analysis, AnalysisState::Frame);
    assert_eq!(record.image.unwrap().bytes(), 8);
    let mut wrong = end(&id);
    wrong["images"] = json!(0);
    assert!(accept(&mut state, wrong).is_err());
    accept(&mut state, end(&id)).unwrap();
    assert!(accept(&mut state, end(&id)).is_err());
    assert_eq!(state.summary().unwrap().images, 1);
    let mut zero = bounds();
    zero.max_images = 0;
    let mut state = wire::State::new(&id, &zero, 2);
    accept(&mut state, ready(&id)).unwrap();
    accept(&mut state, observation(&id, 1, false)).unwrap();
    assert!(accept(&mut state, observation(&id, 2, true)).is_err());
}

async fn send(writer: &mut (impl tokio::io::AsyncWrite + Unpin), value: Value) {
    writer
        .write_all(&(value.to_string() + "\n").into_bytes())
        .await
        .unwrap();
}
async fn line(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> Value {
    let mut value = String::new();
    reader.read_line(&mut value).await.unwrap();
    serde_json::from_str(&value).unwrap()
}

#[tokio::test]
async fn private_wire_starts_after_ready_and_acknowledges_only_after_consumer_returns() {
    let id = id();
    let limits = bounds();
    let (parent_out, child_in) = tokio::io::duplex(8192);
    let (child_out, parent_in) = tokio::io::duplex(8192);
    let (seen, received) = tokio::sync::oneshot::channel();
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let admitted = gate.clone();
    let mut seen = Some(seen);
    let operation = wire::exchange(
        parent_out,
        parent_in,
        b"{}\n".to_vec(),
        &id,
        &limits,
        2,
        move |_| {
            let gate = admitted.clone();
            if let Some(seen) = seen.take() {
                let _ = seen.send(());
            }
            async move {
                gate.acquire().await.unwrap().forget();
                Ok(())
            }
        },
    );
    let peer = async {
        let mut reader = BufReader::new(child_in);
        let mut writer = child_out;
        assert_eq!(line(&mut reader).await, json!({}));
        send(&mut writer, ready(&id)).await;
        assert_eq!(
            line(&mut reader).await,
            json!({"type":"start","schema_version":1,"stream_id":id})
        );
        send(&mut writer, observation(&id, 1, false)).await;
        received.await.unwrap();
        let mut byte = [0];
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                tokio::io::AsyncReadExt::read(&mut reader, &mut byte)
            )
            .await
            .is_err()
        );
        gate.add_permits(2);
        assert_eq!(line(&mut reader).await["sequence"], 1);
        send(&mut writer, observation(&id, 2, true)).await;
        assert_eq!(line(&mut reader).await["sequence"], 2);
        send(&mut writer, end(&id)).await;
    };
    let (summary, ()) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(operation, peer)
    })
    .await
    .unwrap();
    assert_eq!(summary.unwrap().images, 1);
}

#[tokio::test]
async fn failed_private_consumer_receives_no_ack_and_exposes_no_private_error() {
    let id = id();
    let limits = bounds();
    let (parent_out, child_in) = tokio::io::duplex(8192);
    let (child_out, parent_in) = tokio::io::duplex(8192);
    let operation = wire::exchange(
        parent_out,
        parent_in,
        b"{}\n".to_vec(),
        &id,
        &limits,
        2,
        |_| async { bail!("PRIVATE-consumer-data") },
    );
    let peer = async {
        let mut reader = BufReader::new(child_in);
        let mut writer = child_out;
        line(&mut reader).await;
        send(&mut writer, ready(&id)).await;
        line(&mut reader).await;
        send(&mut writer, observation(&id, 1, false)).await;
        let mut value = String::new();
        assert_eq!(reader.read_line(&mut value).await.unwrap(), 0);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(operation, peer)
    })
    .await
    .unwrap();
    assert!(!result.unwrap_err().to_string().contains("PRIVATE"));
}

#[cfg(not(windows))]
#[tokio::test]
async fn unsupported_owned_vision_refuses_before_interpreter_start_or_consumer() {
    let runtime = PythonRuntime {
        executable: std::path::PathBuf::from("PRIVATE-must-not-start"),
        limits: BridgeLimits::default(),
    };
    let bytes = b"def stream(action,params):\n    yield params\n";
    let source = PinnedStreamSource::new(bytes, &hex::encode(Sha256::digest(bytes))).unwrap();
    let pool = crate::capture_ownership::Pool::camera();
    let error = runtime
        .observe_reserved_vision(
            &source,
            json!({}),
            &id(),
            bounds(),
            Instant::now() + Duration::from_secs(1),
            pool.reserve().unwrap(),
            |_| async {
                panic!("Unsupported vision ran a consumer");
                #[allow(unreachable_code)]
                Ok(())
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unsupported on this platform"));
    assert!(!error.to_string().contains("PRIVATE"));
    assert!(pool.reserve().is_ok());
}

#[tokio::test]
async fn private_wire_rejects_truncation_line_and_aggregate_overflow_without_completion() {
    let id = id();
    let mut limits = bounds();
    limits.max_stdout_bytes = limits.max_frame_bytes;
    limits.validate(&BridgeLimits::default()).unwrap();
    let first = observation(&id, 1, false).to_string() + "\n";
    let second = observation(&id, 2, true).to_string() + "\n";
    let oversized = " ".repeat(limits.max_frame_bytes) + "\n";
    let padded = first.trim_end().to_owned() + &" ".repeat(1600) + "\n" + &second;
    for output in [first.clone(), first.trim_end().into(), oversized, padded] {
        let (parent_out, child_in) = tokio::io::duplex(8192);
        let (child_out, parent_in) = tokio::io::duplex(8192);
        let operation = wire::exchange(
            parent_out,
            parent_in,
            b"{}\n".to_vec(),
            &id,
            &limits,
            2,
            |_| async { Ok(()) },
        );
        let peer = async {
            let mut reader = BufReader::new(child_in);
            let mut writer = child_out;
            line(&mut reader).await;
            send(&mut writer, ready(&id)).await;
            line(&mut reader).await;
            writer.write_all(output.as_bytes()).await.unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(operation, peer)
        })
        .await
        .unwrap();
        assert!(result.is_err(), "Incomplete or oversized wire accepted");
    }
}

#[cfg(all(windows, feature = "native-test-fixtures"))]
#[path = "python_vision_native_tests.rs"]
mod native;
