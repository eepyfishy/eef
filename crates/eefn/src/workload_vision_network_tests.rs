use super::*;
use crate::workload_leases::definition::{SourceKind, vision::tests::vision};
use serde_json::json;

fn policy() -> Policy {
    Policy {
        enabled: true,
        vision_analysis: true,
        ..Default::default()
    }
}
fn registered(runtime: &str) -> Value {
    json!({
        "registration_receipt":crate::registration::Offer::new(runtime).unwrap(),
        "execution_grants":{"available":true},
        "workload_execution":crate::workload_network::Advertisement::for_sources(runtime,policy(),vec![SourceKind::CameraMotion]).unwrap(),
        "workload_vision_execution":Advertisement::for_runtime(runtime,policy(),true).unwrap(),
        "workload_vision_results":crate::vision_result_inspection::Advertisement::new(true),
        "grant_stop":crate::grants::stop::Advertisement::new(true)
    })
}

#[test]
fn vision_negotiation_is_distinct_and_requires_current_owner_configuration() {
    let runtime = uuid::Uuid::new_v4().to_string();
    for (p, ready) in [
        (Policy::default(), true),
        (policy(), false),
        (
            Policy {
                vision_analysis: false,
                ..policy()
            },
            true,
        ),
    ] {
        let support = Advertisement::for_runtime(&runtime, p, ready).unwrap();
        assert_eq!(json!(support), json!(Advertisement::unavailable()));
        assert!(support.confirms(&vision().source, &runtime).is_err());
    }
    let support = Advertisement::for_runtime(&runtime, policy(), true).unwrap();
    support.confirms(&vision().source, &runtime).unwrap();
    assert!(
        support
            .confirms(&vision().source, &uuid::Uuid::new_v4().to_string())
            .is_err()
    );
    let metadata = crate::workload_leases::definition::camera_tests::camera();
    assert!(support.confirms(&metadata.source, &runtime).is_err());
    let old = crate::workload_network::Advertisement::for_sources(
        &runtime,
        policy(),
        vec![SourceKind::CameraMotion],
    )
    .unwrap();
    assert!(!old.policy.as_ref().unwrap().vision_analysis);
    assert!(json!(old)["policy"].get("vision_analysis").is_none());
    assert!(old.confirms_source(&vision().source).is_err());
}

#[test]
fn partial_or_unknown_vision_contracts_cannot_claim_support() {
    let runtime = uuid::Uuid::new_v4().to_string();
    let support = Advertisement::for_runtime(&runtime, policy(), true).unwrap();
    for (pointer, value) in [
        ("/schema_version", json!(2)),
        ("/definition_schema_version", json!(2)),
        ("/offer_schema_version", json!(2)),
        ("/completion_schema_version", json!(2)),
        ("/results_schema_version", json!(2)),
        ("/private_images", json!(false)),
        ("/restart_authorized", json!(true)),
        ("/source_kind", json!("camera_motion")),
        ("/policy/vision_analysis", json!(false)),
        ("/available", json!(false)),
        ("/runtime_id", Value::Null),
    ] {
        let mut value_to_check = json!(support);
        *value_to_check.pointer_mut(pointer).unwrap() = value;
        let parsed: Advertisement = serde_json::from_value(value_to_check).unwrap();
        assert!(parsed.validate().is_err(), "{pointer}");
    }
    let mut unknown = json!(support);
    unknown["fallback"] = json!(true);
    assert!(serde_json::from_value::<Advertisement>(unknown).is_err());
}

#[test]
fn vision_placement_requires_exact_runtime_journal_stop_and_analysis_protocols_without_fallback() {
    let runtime = uuid::Uuid::new_v4().to_string();
    let state = registered(&runtime);
    let source = vision().source;
    assert_eq!(registered_support(&state, &source).unwrap().0, runtime);
    for path in [
        "/workload_vision_execution",
        "/workload_vision_results",
        "/grant_stop",
        "/execution_grants/available",
    ] {
        let mut incomplete = state.clone();
        *incomplete.pointer_mut(path).unwrap() = Value::Null;
        assert!(registered_support(&incomplete, &source).is_err(), "{path}");
    }
    for path in [
        "/workload_vision_results/available",
        "/grant_stop/available",
        "/workload_vision_execution/available",
    ] {
        let mut unavailable = state.clone();
        *unavailable.pointer_mut(path).unwrap() = json!(false);
        assert!(registered_support(&unavailable, &source).is_err(), "{path}");
    }
    let mut retired = state.clone();
    retired["registration_receipt"]["runtime_id"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(registered_support(&retired, &source).is_err());
    let mut old = state;
    old.as_object_mut()
        .unwrap()
        .remove("workload_vision_execution");
    assert!(registered_support(&old, &source).is_err());
    let metadata = crate::workload_leases::definition::camera_tests::camera();
    assert_eq!(
        registered_support(&old, &metadata.source).unwrap().0,
        runtime
    );
}

#[test]
fn offer_version_cannot_upgrade_a_metadata_grant_or_downgrade_a_vision_grant() {
    let runtime = uuid::Uuid::new_v4().to_string();
    for d in [
        vision(),
        crate::workload_leases::definition::camera_tests::camera(),
    ] {
        let offer = crate::workload_network::Offer {
            kind: "execute_workload".into(),
            schema_version: d.schema_version,
            id: d.grant_id.clone(),
            expected_runtime_id: runtime.clone(),
            definition: d.clone(),
        };
        offer
            .validate(&d.issuer_id, &d.session_id, &d.executor_node, &runtime)
            .unwrap();
        for version in [1, 2, 3, 4] {
            let mut changed = json!(offer);
            changed["schema_version"] = json!(version);
            let parsed: crate::workload_network::Offer = serde_json::from_value(changed).unwrap();
            assert_eq!(
                parsed
                    .validate(&d.issuer_id, &d.session_id, &d.executor_node, &runtime)
                    .is_ok(),
                version == d.schema_version
            );
        }
    }
}
