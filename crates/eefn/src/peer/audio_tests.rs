use super::*;
use crate::grants::{GrantReport, ReceiptState};
use crate::peer::{
    admission::{Definition, TransferReceipt},
    scope::Limits,
};
fn audio() -> Audio {
    Audio {
        duration_ms: 500,
        sample_rate: 16000,
        channels: 1,
        source_resource_id: "source::microphone".into(),
        destination_resource_id: "destination::speaker".into(),
    }
}
fn wave(spec: &Audio) -> Vec<u8> {
    let frames = spec.duration_ms * spec.sample_rate / 1000;
    let length = frames * u32::from(spec.channels) * 2;
    let mut bytes = vec![];
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&u16::from(spec.channels).to_le_bytes());
    bytes.extend_from_slice(&spec.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(spec.sample_rate * u32::from(spec.channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(u16::from(spec.channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.resize(bytes.len() + length as usize, 0);
    bytes
}
fn definition() -> Definition {
    let spec = audio();
    Definition {
        schema_version: 2,
        resource_id: "source::transport".into(),
        peer_node: "destination".into(),
        peer_resource_id: "destination::transport".into(),
        payload_bytes: spec.maximum_bytes(),
        payload_sha256: None,
        audio: Some(spec),
        limits: Limits {
            max_wire_bytes: 1024 * 1024,
            max_bytes_per_second: 1024 * 1024,
            max_frame_bytes: 512,
            max_frames: 256,
            max_inflight_frames: 1,
        },
    }
}
fn grant() -> crate::grants::ExecutionGrant {
    let mut grant = crate::grants::tests::grant();
    grant.executor_node = "source".into();
    grant.resource_id = Some("source::transport".into());
    grant.capability = "peer.transfer".into();
    grant.action = "send".into();
    grant.params = json!(definition());
    grant
}
#[test]
fn finite_audio_definition_binds_resources_format_duration_and_no_pretend_hash() {
    let spec = audio();
    spec.validate_nodes("source", "destination").unwrap();
    assert!(spec.validate_nodes("other", "destination").is_err());
    let g = grant();
    g.validate().unwrap();
    let mut d = definition();
    d.payload_sha256 = Some("a".repeat(64));
    assert!(d.validate().is_err());
    d = definition();
    d.schema_version = 1;
    assert!(d.validate().is_err());
    d = definition();
    d.payload_bytes += 1;
    assert!(d.validate().is_err());
    d = definition();
    d.limits.max_frame_bytes = 511;
    assert!(d.validate().is_err());
    let mut short = g.clone();
    short.limits.max_execution_ms = spec.minimum_execution_ms() - 1;
    assert!(short.validate().is_err());
    for bad in [
        Audio {
            duration_ms: 99,
            ..audio()
        },
        Audio {
            duration_ms: 10001,
            ..audio()
        },
        Audio {
            sample_rate: 96000,
            ..audio()
        },
        Audio {
            channels: 3,
            ..audio()
        },
    ] {
        assert!(bad.validate().is_err());
    }
}
#[test]
fn wav_parser_rejects_wrong_format_truncation_duplicate_chunks_and_wrong_duration() {
    let spec = audio();
    let bytes = wave(&spec);
    spec.validate_wav(&bytes).unwrap();
    for (at, value) in [
        (0, b'X'),
        (8, b'X'),
        (20, 3),
        (22, 2),
        (24, 0),
        (28, 1),
        (32, 7),
        (34, 8),
        (40, 1),
    ] {
        let mut wrong = bytes.clone();
        wrong[at] = value;
        assert!(spec.validate_wav(&wrong).is_err(), "field {at}");
    }
    assert!(spec.validate_wav(&bytes[..bytes.len() - 1]).is_err());
    assert!(
        Audio {
            duration_ms: 1000,
            ..audio()
        }
        .validate_wav(&bytes)
        .is_err()
    );
    let mut duplicate = bytes.clone();
    duplicate.extend_from_slice(b"data\0\0\0\0");
    let n = (duplicate.len() - 8) as u32;
    duplicate[4..8].copy_from_slice(&n.to_le_bytes());
    assert!(spec.validate_wav(&duplicate).is_err());
}
#[test]
fn audio_receipts_require_actual_bounded_content_and_exact_endpoint_role() {
    let g = grant();
    let n = wave(&audio()).len() as u64;
    let receipt = TransferReceipt {
        schema_version: 2,
        pair_session_id: uuid::Uuid::new_v4().to_string(),
        scope_sha256: "b".repeat(64),
        role: Role::Source,
        payload_bytes: n,
        payload_sha256: "a".repeat(64),
        data_frames: 1 + n.div_ceil(512) as u32,
        payload_retained: false,
        sink_effects_confirmed: false,
        producer_reported: true,
        sink_reported: false,
    };
    let report = GrantReport::new(&g, ReceiptState::Completed, json!(receipt), None);
    assert!(report.confirms(&g));
    for (pointer, value) in [
        ("/payload_bytes", json!(1)),
        ("/schema_version", json!(1)),
        ("/producer_reported", json!(false)),
        ("/sink_reported", json!(true)),
        ("/sink_effects_confirmed", json!(true)),
        ("/data_frames", json!(1)),
        ("/payload_sha256", json!("bad")),
    ] {
        let mut wrong = report.clone();
        wrong
            .data
            .as_object_mut()
            .unwrap()
            .insert(pointer.trim_start_matches('/').into(), value);
        assert!(!wrong.confirms(&g), "{pointer}");
    }
}

#[test]
fn transport_strips_metadata_and_refuses_noncanonical_sink_content() {
    let spec = audio();
    let canonical = wave(&spec);
    spec.validate_transport_wav(&canonical).unwrap();
    let mut with_metadata = canonical[..36].to_vec();
    with_metadata.extend_from_slice(b"JUNK\x04\x00\x00\x00TEST");
    with_metadata.extend_from_slice(&canonical[36..]);
    let size = (with_metadata.len() - 8) as u32;
    with_metadata[4..8].copy_from_slice(&size.to_le_bytes());
    spec.validate_wav(&with_metadata).unwrap();
    assert!(spec.validate_transport_wav(&with_metadata).is_err());
    assert_eq!(spec.canonicalize(with_metadata).unwrap(), canonical);
}
