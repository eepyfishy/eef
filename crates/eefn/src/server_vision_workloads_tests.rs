// Route preparation/offer versioning only; no synthetic value grants a source lease.
#[tokio::test]
async fn vision_route_requires_distinct_support_and_never_falls_back_to_metadata_offer() {
    let (server, mut d, mut output) = fixture().await;
    let vision = crate::workload_leases::definition::vision::tests::vision();
    d.schema_version = 3;
    d.source = vision.source;
    d.limits = vision.limits;
    d.events = vision.events;
    if let Source::CameraVision { resource_id, .. } = &mut d.source {
        *resource_id = "owner::camera".into();
    }
    d.validate().unwrap();
    assert!(
        server
            .prepare_workload(d.clone(), deadline())
            .await
            .is_err()
    );
    assert!(output.try_recv().is_err());
    let original = server.inner.connections.read().await["owner"]
        .state
        .as_ref()
        .unwrap()
        .registration
        .clone();
    let runtime = original["registration_receipt"]["runtime_id"]
        .as_str()
        .unwrap();
    let mut supported = original.clone();
    supported["workload_vision_execution"] = json!(
        crate::workload_vision_network::Advertisement::for_runtime(
            runtime,
            crate::workload_leases::Policy {
                enabled: true,
                vision_analysis: true,
                ..Default::default()
            },
            true,
        )
        .unwrap()
    );
    supported["workload_vision_results"] =
        json!(crate::vision_result_inspection::Advertisement::new(true));
    supported["grant_stop"] = json!(crate::grants::stop::Advertisement::new(true));
    supported["execution_grants"] = json!({"available":true});
    for (pointer, value) in [
        ("/workload_vision_results/available", json!(false)),
        ("/grant_stop/available", json!(false)),
        ("/workload_vision_execution/offer_schema_version", json!(2)),
        (
            "/workload_vision_execution/runtime_id",
            json!(Uuid::new_v4().to_string()),
        ),
        ("/execution_grants/available", json!(false)),
    ] {
        let mut changed = supported.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        server
            .inner
            .connections
            .write()
            .await
            .get_mut("owner")
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .registration = changed;
        assert!(
            server
                .prepare_workload(d.clone(), deadline())
                .await
                .is_err(),
            "{pointer}"
        );
        assert!(output.try_recv().is_err());
        assert!(server.inner.workloads.0.lock().unwrap().is_empty());
    }
    server
        .inner
        .connections
        .write()
        .await
        .get_mut("owner")
        .unwrap()
        .state
        .as_mut()
        .unwrap()
        .registration = supported;
    let mut link = server
        .prepare_workload(d.clone(), deadline())
        .await
        .unwrap();
    assert!(
        output.try_recv().is_err(),
        "Preparing a route cannot dispatch execution"
    );
    link.dispatch_offer().unwrap();
    let raw = output.recv().await.unwrap();
    assert_eq!(raw["schema_version"], 3);
    let offer: Offer = serde_json::from_value(raw).unwrap();
    offer
        .validate(&d.issuer_id, &d.session_id, "owner", runtime)
        .unwrap();
    assert_eq!(offer.definition, d);
    assert!(link.dispatch_offer().is_err(), "Offer cannot be sent twice");
    drop(link);
    assert!(server.inner.workloads.0.lock().unwrap().is_empty());
    // The original metadata registration was not expanded or rewritten.
    assert_eq!(
        server.inner.connections.read().await["owner"]
            .state
            .as_ref()
            .unwrap()
            .registration["workload_execution"],
        original["workload_execution"]
    );
}
