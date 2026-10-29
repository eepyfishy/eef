use super::*;

fn definition() -> Definition {
    Definition {
        schema_version: 2,
        grant_id: uuid::Uuid::new_v4().to_string(),
        issuer_id: "a".repeat(64),
        session_id: uuid::Uuid::new_v4().to_string(),
        executor_node: "node-b".into(),
        job_id: "b".repeat(32),
        task_id: "c".repeat(32),
        attempt: 1,
        plan_sha256: "d".repeat(64),
        request_context: crate::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "node-a".into(),
            vec!["home".into(), "bedroom".into()],
        )
        .unwrap(),
        priority: 0,
        source: Source::MicrophoneEnergy {
            resource_id: "node-b::microphone".into(),
            resource_sha256: "e".repeat(64),
            adapter_sha256: "f".repeat(64),
            parameters: EnergyParameters {
                sample_rate: 16000,
                channels: 1,
                block_frames: 320,
                max_blocks: 1000,
                activation_level: 1000,
                deactivation_level: 500,
            },
        },
        limits: Limits {
            max_session_ms: 60_000,
            max_lease_ms: 10_000,
            max_job_memory_mib: 128,
            max_source_frames: 320_000,
        },
        events: Events {
            mode: EventMode::ActivityTransitions,
            min_interval_ms: 100,
            max_events: 100,
            max_event_bytes: 1024,
            max_total_bytes: 64 * 1024,
            max_pending: 4,
            overflow: Overflow::Stop,
        },
    }
}
fn decode(value: serde_json::Value) -> anyhow::Result<Definition> {
    let definition: Definition = serde_json::from_value(value)?;
    definition.validate()?;
    Ok(definition)
}
#[test]
fn exact_definition_binds_origin_resource_source_limits_and_reporting_to_lease() {
    let d = definition();
    let scope = d.scope().unwrap();
    assert_eq!(
        scope.grant_sha256,
        crate::grants::definition_digest(&json!(d)).unwrap()
    );
    let contract = d.lease_contract().unwrap();
    contract.validate().unwrap();
    assert_eq!(contract.scope, scope);
    assert_eq!(contract.max_session_ms, 60_000);
    assert_eq!(contract.max_lease_ms, 10_000);
    // Each change is still structurally valid, but cannot inherit a lease for the
    // original reviewed definition. Source frames are bounded on returned reads.
    for (pointer, value) in [
        ("/grant_id", json!(uuid::Uuid::new_v4().to_string())),
        ("/issuer_id", json!("1".repeat(64))),
        ("/session_id", json!(uuid::Uuid::new_v4().to_string())),
        ("/job_id", json!("2".repeat(32))),
        ("/task_id", json!("3".repeat(32))),
        ("/attempt", json!(2)),
        ("/plan_sha256", json!("4".repeat(64))),
        ("/priority", json!(1)),
        (
            "/request_context/request_id",
            json!(uuid::Uuid::new_v4().to_string()),
        ),
        ("/request_context/origin_node", json!("node-c")),
        (
            "/request_context/origin_area",
            json!(["home", "living-room"]),
        ),
        ("/source/resource_id", json!("node-b::other-microphone")),
        ("/source/resource_sha256", json!("5".repeat(64))),
        ("/source/adapter_sha256", json!("6".repeat(64))),
        ("/source/parameters/sample_rate", json!(48000)),
        ("/source/parameters/channels", json!(2)),
        ("/source/parameters/block_frames", json!(160)),
        ("/source/parameters/max_blocks", json!(500)),
        ("/source/parameters/activation_level", json!(2000)),
        ("/source/parameters/deactivation_level", json!(100)),
        ("/limits/max_session_ms", json!(120_000)),
        ("/limits/max_lease_ms", json!(20_000)),
        ("/limits/max_job_memory_mib", json!(256)),
        ("/limits/max_source_frames", json!(640_000)),
        ("/events/min_interval_ms", json!(200)),
        ("/events/max_events", json!(200)),
        ("/events/max_event_bytes", json!(2048)),
        ("/events/max_total_bytes", json!(128 * 1024)),
        ("/events/max_pending", json!(8)),
    ] {
        let mut raw = json!(d);
        *raw.pointer_mut(pointer).unwrap() = value;
        let changed = decode(raw).unwrap_or_else(|e| panic!("{pointer}: {e}"));
        assert_ne!(
            changed.scope().unwrap().grant_sha256,
            scope.grant_sha256,
            "{pointer}"
        );
        assert_ne!(
            changed.lease_contract().unwrap().digest().unwrap(),
            contract.digest().unwrap(),
            "{pointer}"
        );
    }
    let mut changed = d.clone();
    changed.executor_node = "node-c".into();
    let Source::MicrophoneEnergy { resource_id, .. } = &mut changed.source;
    *resource_id = "node-c::microphone".into();
    assert_ne!(changed.scope().unwrap(), scope);
}
#[test]
fn strict_schema_refuses_hidden_authority_and_unimplemented_sources() {
    let d = definition();
    for pointer in [
        "",
        "/source",
        "/source/parameters",
        "/limits",
        "/events",
        "/request_context",
    ] {
        let mut raw = json!(d);
        raw.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unreviewed".into(), json!("PRIVATE"));
        assert!(decode(raw).is_err(), "{pointer}");
    }
    for (pointer, value) in [
        ("/schema_version", json!(1)),
        ("/source/kind", json!("python")),
        ("/source/kind", json!("camera")),
        ("/events/mode", json!("raw_audio")),
        ("/events/overflow", json!("unbounded_buffer")),
    ] {
        let mut raw = json!(d);
        *raw.pointer_mut(pointer).unwrap() = value;
        assert!(decode(raw).is_err(), "{pointer}");
    }
    // New sessions cannot be parsed/consumed as legacy one-operation grants.
    assert!(serde_json::from_value::<crate::grants::ExecutionGrant>(json!(d)).is_err());
    for field in [
        "request_context",
        "source",
        "limits",
        "events",
        "plan_sha256",
    ] {
        let mut raw = json!(d);
        raw.as_object_mut().unwrap().remove(field);
        assert!(decode(raw).is_err(), "{field}");
    }
}
#[test]
fn invalid_identity_resource_or_quota_cannot_produce_a_contract() {
    let d = definition();
    for (pointer, value) in [
        ("/grant_id", json!(uuid::Uuid::nil().to_string())),
        ("/session_id", json!(uuid::Uuid::nil().to_string())),
        ("/attempt", json!(0)),
        ("/attempt", json!(12)),
        ("/issuer_id", json!("A".repeat(64))),
        ("/plan_sha256", json!("not a pin")),
        ("/priority", json!(1001)),
        ("/request_context/origin_node", json!("*")),
        (
            "/request_context/request_id",
            json!(uuid::Uuid::nil().to_string()),
        ),
        ("/request_context/origin_area", json!(["bad/path"])),
        ("/source/resource_id", json!("node-a::microphone")),
        ("/source/resource_id", json!("node-b::")),
        ("/source/resource_id", json!("node-b::microphone::alias")),
        ("/source/resource_sha256", json!("")),
        ("/source/adapter_sha256", json!("")),
        ("/source/parameters/sample_rate", json!(0)),
        ("/source/parameters/channels", json!(3)),
        ("/source/parameters/block_frames", json!(4097)),
        ("/source/parameters/max_blocks", json!(100001)),
        ("/source/parameters/activation_level", json!(32769)),
        ("/source/parameters/deactivation_level", json!(1001)),
        ("/limits/max_session_ms", json!(MAX_SESSION_MS + 1)),
        ("/limits/max_lease_ms", json!(MAX_LEASE_MS + 1)),
        ("/limits/max_job_memory_mib", json!(0)),
        ("/limits/max_job_memory_mib", json!(63)),
        (
            "/limits/max_job_memory_mib",
            json!(crate::model_memory::MAX_JOB_MEMORY_MIB + 1),
        ),
        ("/limits/max_source_frames", json!(319_999)),
        ("/events/min_interval_ms", json!(0)),
        ("/events/max_events", json!(0)),
        ("/events/max_event_bytes", json!(4097)),
        ("/events/max_total_bytes", json!(0)),
        ("/events/max_pending", json!(65)),
    ] {
        let mut raw = json!(d);
        *raw.pointer_mut(pointer).unwrap() = value;
        assert!(decode(raw).is_err(), "{pointer}");
    }
}
fn private_binding() -> (Definition, crate::context::Resource) {
    let mut d = definition();
    let resource = crate::context::Resource {
        id: "microphone".into(),
        name: "Local microphone".into(),
        capability: "audio.capture".into(),
        area: vec![],
        available: true,
        parameters: Default::default(),
        properties: Default::default(),
    };
    pin(&mut d, &resource);
    (d, resource)
}
fn pin(d: &mut Definition, resource: &crate::context::Resource) {
    let Source::MicrophoneEnergy {
        resource_sha256,
        adapter_sha256,
        ..
    } = &mut d.source;
    *resource_sha256 = crate::grants::definition_digest(&json!(resource)).unwrap();
    *adapter_sha256 = hex::encode(Sha256::digest(b"reviewed adapter bytes"));
}
#[test]
fn private_device_stays_local_and_normalized_request_matches_existing_iterator() {
    let (mut d, mut resource) = private_binding();
    let original = d.scope().unwrap();
    for device in [
        serde_json::Value::Null,
        json!(5),
        json!("PRIVATE microphone name"),
    ] {
        resource.parameters.insert("device".into(), device.clone());
        // Local owner review creates a new resource pin, never changes a live
        // lease to follow a different private microphone selection.
        pin(&mut d, &resource);
        let request = d
            .bound_python_parameters(&resource, b"reviewed adapter bytes")
            .unwrap();
        assert_eq!(request["device"], device);
        assert_eq!(request.as_object().unwrap().len(), 7);
        assert_eq!(request["sample_rate"], 16000);
        assert_eq!(request["block_frames"], 320);
        assert_eq!(request["max_blocks"], 1000);
        assert!(request.get("resource_id").is_none());
        assert!(!serde_json::to_string(&d).unwrap().contains("PRIVATE"));
        assert!(
            !serde_json::to_string(&d.lease_contract().unwrap())
                .unwrap()
                .contains("PRIVATE")
        );
        assert_ne!(d.scope().unwrap(), original);
    }
}
#[test]
fn actual_resource_and_adapter_drift_or_invalid_private_binding_are_refused() {
    let (d, resource) = private_binding();
    d.bound_python_parameters(&resource, b"reviewed adapter bytes")
        .unwrap();
    assert!(
        d.bound_python_parameters(&resource, b"substituted adapter")
            .is_err()
    );
    for field in ["id", "capability", "available", "parameters"] {
        let mut raw = json!(resource);
        raw[field] = match field {
            "id" => json!("other"),
            "capability" => json!("audio.play"),
            "available" => json!(false),
            _ => json!({"device":9}),
        };
        let changed = serde_json::from_value(raw).unwrap();
        assert!(
            d.bound_python_parameters(&changed, b"reviewed adapter bytes")
                .is_err(),
            "{field}"
        );
    }
    for device in [
        json!(-1),
        json!(1024),
        json!(1.5),
        json!(true),
        json!([]),
        json!({}),
        json!(" "),
        json!("bad\nname"),
    ] {
        let mut changed = resource.clone();
        changed.parameters.insert("device".into(), device);
        let mut approved = d.clone();
        pin(&mut approved, &changed);
        assert!(
            approved
                .bound_python_parameters(&changed, b"reviewed adapter bytes")
                .is_err()
        );
    }
    let mut raw = json!(d);
    raw["source"]["parameters"]["device"] = json!(9);
    assert!(
        decode(raw).is_err(),
        "Private binding must not enter the shared definition"
    );
    let mut changed = resource.clone();
    changed
        .parameters
        .insert("output".into(), json!("unreviewed"));
    let mut approved = d;
    pin(&mut approved, &changed);
    assert!(
        approved
            .bound_python_parameters(&changed, b"reviewed adapter bytes")
            .is_err()
    );
}
#[test]
fn full_definition_contract_prevents_renewing_changed_origin_or_capture() {
    let d = definition();
    let c = d.lease_contract().unwrap();
    let policy = super::super::Policy {
        enabled: true,
        ..Default::default()
    };
    let mut issuer = super::super::IssuerWindow::new(
        c.clone(),
        &policy,
        std::time::Instant::now() + std::time::Duration::from_millis(50_000),
    )
    .unwrap();
    let mut changed = json!(d);
    changed["source"]["resource_sha256"] = json!("9".repeat(64));
    let substitute = decode(changed).unwrap().lease_contract().unwrap();
    let q = super::super::Query {
        kind: "workload_lease_probe".into(),
        schema_version: 2,
        id: uuid::Uuid::new_v4().to_string(),
        contract: substitute,
        sequence: 1,
    };
    assert!(
        issuer
            .reply(
                &q,
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
    let q = super::super::Query {
        contract: c.clone(),
        ..q
    };
    assert!(
        issuer
            .reply(
                &q,
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_ok()
    );
}
