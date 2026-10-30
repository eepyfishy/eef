use super::*;
use tokio::io::AsyncWriteExt;

#[test]
fn pinned_source_owns_exact_utf8_bytes_without_reopening_or_serializing_paths() {
    let mut bytes = b"def stream(action, params):\n    yield params\n".to_vec();
    let digest = hex::encode(Sha256::digest(&bytes));
    let source = PinnedStreamSource::new(&bytes, &digest).unwrap();
    bytes.fill(b'x');
    assert_eq!(source.digest(), digest);
    assert_eq!(
        hex::encode(Sha256::digest(source.source_utf8.as_bytes())),
        digest
    );
    assert!(!source.source_utf8.contains('x'));
    assert!(PinnedStreamSource::new(&bytes, &digest).is_err());
}

#[test]
fn pinned_source_rejects_bad_digest_encoding_and_size_before_a_process_can_start() {
    for bytes in [vec![], vec![b'x'; 48 * 1024 + 1], vec![0], vec![255]] {
        let digest = hex::encode(Sha256::digest(&bytes));
        assert!(PinnedStreamSource::new(&bytes, &digest).is_err());
    }
    let bytes = b"def stream(action, params):\n    yield params\n";
    let digest = hex::encode(Sha256::digest(bytes));
    for changed in [
        digest.to_uppercase(),
        "a".repeat(63),
        "g".repeat(64),
        "0".repeat(64),
    ] {
        assert!(PinnedStreamSource::new(bytes, &changed).is_err());
    }
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn ready(id: &str) -> Value {
    json!({"type":"ready","schema_version":1,"stream_id":id})
}
fn observation(id: &str, n: u64, frames: u64) -> Value {
    json!({"type":"observation","stream_id":id,"sequence":n,"source_frames":frames,
        "active":false,"level":7,"discontinuity":false})
}
fn end(id: &str, n: u64, frames: u64) -> Value {
    json!({"type":"end","stream_id":id,"observations":n,"source_frames":frames})
}
fn wire(values: &[Value]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| {
            let mut b = serde_json::to_vec(v).unwrap();
            b.push(b'\n');
            b
        })
        .collect()
}
async fn parse(values: &[Value], id: &str, bounds: &StreamBounds) -> Result<StreamSummary> {
    let bytes = wire(values);
    read_frames(bytes.as_slice(), id, bounds, |_| async { Ok(()) }).await
}

#[test]
fn stream_bounds_tighten_existing_limits_without_adding_authority() {
    let local = BridgeLimits::default();
    StreamBounds::default().validate(&local).unwrap();
    for bounds in [
        StreamBounds {
            max_frame_bytes: 127,
            ..Default::default()
        },
        StreamBounds {
            max_frame_bytes: 16385,
            ..Default::default()
        },
        StreamBounds {
            max_stdout_bytes: 1023,
            ..Default::default()
        },
        StreamBounds {
            max_observations: 0,
            ..Default::default()
        },
        StreamBounds {
            max_observations: 100001,
            ..Default::default()
        },
        StreamBounds {
            max_source_frames: 0,
            ..Default::default()
        },
    ] {
        assert!(bounds.validate(&local).is_err());
    }
    let local = BridgeLimits {
        max_stdout_bytes: 8192,
        ..Default::default()
    };
    assert!(StreamBounds::default().validate(&local).is_err());
    assert!(crate::execution_boundary::require_control("stream.observe").is_err());
    assert!(!crate::engine::CAPABILITIES.contains(&"stream.observe"));
}

#[tokio::test]
async fn observation_is_delivered_before_end_or_eof_and_unknown_is_not_quiet() {
    let id = id();
    let bounds = StreamBounds::default();
    let (mut producer, consumer) = tokio::io::duplex(2048);
    let mut first = observation(&id, 1, 80);
    first["active"] = Value::Null;
    first["level"] = Value::Null;
    first["discontinuity"] = json!(true);
    producer
        .write_all(&wire(&[ready(&id), first]))
        .await
        .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    let read = read_frames(consumer, &id, &bounds, move |value| {
        let sender = tx.take();
        async move {
            sender.unwrap().send(value).unwrap();
            Ok(())
        }
    });
    tokio::pin!(read);
    let event = tokio::select! {v=rx=>v.unwrap(),_=&mut read=>panic!("Must wait for end/EOF after live delivery")};
    assert_eq!(event.active, None);
    assert_eq!(event.level, None);
    assert!(event.discontinuity);
    producer.write_all(&wire(&[end(&id, 1, 80)])).await.unwrap();
    producer.shutdown().await.unwrap();
    let summary = read.await.unwrap();
    assert_eq!(summary.observations, 1);
    assert_eq!(summary.source_frames, 80);
}

#[tokio::test]
async fn stream_rejects_wrong_scope_order_duplicates_and_false_gap_certainty() {
    let id = id();
    let bounds = StreamBounds::default();
    let good = observation(&id, 1, 80);
    let mut wrong_scope = good.clone();
    wrong_scope["stream_id"] = json!(self::id());
    let mut gap = good.clone();
    gap["discontinuity"] = json!(true);
    let mut private = good.clone();
    private["data_base64"] = json!("PRIVATE_SAMPLE");
    let mut excessive = good.clone();
    excessive["level"] = json!(32769);
    for frames in [
        vec![good.clone(), end(&id, 1, 80)],
        vec![ready(&id), ready(&id), good.clone(), end(&id, 1, 80)],
        vec![ready(&id), wrong_scope, end(&id, 1, 80)],
        vec![ready(&id), gap, end(&id, 1, 80)],
        vec![ready(&id), private, end(&id, 1, 80)],
        vec![ready(&id), excessive, end(&id, 1, 80)],
        vec![ready(&id), good.clone(), good.clone(), end(&id, 1, 80)],
        vec![
            ready(&id),
            good.clone(),
            observation(&id, 2, 80),
            end(&id, 2, 80),
        ],
        vec![ready(&id), good.clone(), end(&id, 2, 80)],
        vec![ready(&id), good.clone(), end(&id, 1, 80), ready(&id)],
    ] {
        assert!(parse(&frames, &id, &bounds).await.is_err());
    }
}

#[tokio::test]
async fn empty_or_unfinished_source_cannot_confirm_a_successful_stream() {
    let id = id();
    let bounds = StreamBounds::default();
    for frames in [
        vec![],
        vec![ready(&id)],
        vec![ready(&id), end(&id, 0, 0)],
        vec![ready(&id), observation(&id, 1, 1)],
    ] {
        assert!(parse(&frames, &id, &bounds).await.is_err());
    }
    let mut bytes = wire(&[ready(&id), observation(&id, 1, 80), end(&id, 1, 80)]);
    bytes.pop();
    assert!(
        read_frames(bytes.as_slice(), &id, &bounds, |_| async { Ok(()) })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn framed_and_aggregate_byte_limits_apply_before_unbounded_allocation() {
    let id = id();
    let bounds = StreamBounds {
        max_frame_bytes: 256,
        ..Default::default()
    };
    let bytes = vec![b'x'; 257];
    let error = read_frames(bytes.as_slice(), &id, &bounds, |_| async { Ok(()) })
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::StdoutLimit)
    );
    let bounds = StreamBounds {
        max_frame_bytes: 1024,
        max_stdout_bytes: 1024,
        ..Default::default()
    };
    let mut frames = vec![ready(&id)];
    for n in 1..=8 {
        frames.push(observation(&id, n, n));
    }
    frames.push(end(&id, 8, 8));
    let error = parse(&frames, &id, &bounds).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::StdoutLimit)
    );
}

#[tokio::test]
async fn count_and_source_frame_bounds_do_not_become_permission_to_drop_records() {
    let id = id();
    let bounds = StreamBounds {
        max_observations: 1,
        ..Default::default()
    };
    assert!(
        parse(
            &[
                ready(&id),
                observation(&id, 1, 1),
                observation(&id, 2, 2),
                end(&id, 2, 2)
            ],
            &id,
            &bounds
        )
        .await
        .is_err()
    );
    let bounds = StreamBounds {
        max_source_frames: 79,
        ..Default::default()
    };
    assert!(
        parse(
            &[ready(&id), observation(&id, 1, 80), end(&id, 1, 80)],
            &id,
            &bounds
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn failed_observer_is_private_uncertainty_not_successful_consumption() {
    let id = id();
    let bounds = StreamBounds::default();
    let bytes = wire(&[ready(&id), observation(&id, 1, 80), end(&id, 1, 80)]);
    let error = read_frames(bytes.as_slice(), &id, &bounds, |_| async {
        bail!("PRIVATE_OBSERVER_DIAGNOSTIC")
    })
    .await
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<BridgeFailure>(),
        Some(&BridgeFailure::Reply)
    );
    assert!(!error.to_string().contains("PRIVATE_OBSERVER"));
}

#[tokio::test]
async fn one_pending_observer_backpressures_later_observations() {
    let id = id();
    let bounds = StreamBounds::default();
    let bytes = wire(&[
        ready(&id),
        observation(&id, 1, 80),
        observation(&id, 2, 160),
        end(&id, 2, 160),
    ]);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = count.clone();
    let read = read_frames(bytes.as_slice(), &id, &bounds, move |_| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let sender = tx.take();
        async move {
            sender.unwrap().send(()).unwrap();
            std::future::pending::<Result<()>>().await
        }
    });
    tokio::pin!(read);
    tokio::select! {_=rx=>{},_=&mut read=>panic!("Observer is still pending")};
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}
