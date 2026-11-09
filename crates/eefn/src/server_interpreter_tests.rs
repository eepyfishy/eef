//! Snapshot validation units; actual private TLS routing is tested in the coordinator.
use super::*;
async fn fixture() -> NodeServer {
    let server = NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([(
                "owner".into(),
                Some(crate::authentication::generate_credential()),
            )]),
            tls: Some(crate::tls::CoordinatorIdentity::generate().unwrap()),
        },
    )
    .unwrap();
    let offer = crate::registration::Offer::new(&Uuid::new_v4().to_string()).unwrap();
    let registration = json!({"node_id":"owner","name":"Snapshot fixture","models":[],"capabilities":[],
        "registration_receipt":offer,"execution_grants":{"schema_version":1,"available":true,
            "timing_mode":crate::grant_time::MODE,"limits":crate::grants::GrantPolicy::default()},
        "interpreter_service":{"schema_version":1,"permission_revision":3}});
    let (sender, _receiver) = mpsc::channel(1);
    server.inner.connections.write().await.insert(
        "owner".into(),
        Connection {
            session_id: Uuid::new_v4(),
            credential_transport: true,
            sender,
            info: ConnectionInfo {
                node_id: "owner".into(),
                source_ip: "PRIVATE_SOURCE".into(),
                source_port: 1234,
                connected_since_ms: 0,
            },
            advertisement: Some(
                crate::network::PeerRecord::from_registration(&registration).unwrap(),
            ),
            state: Some(NodeStateSnapshot {
                registration,
                heartbeat: None,
            }),
            last_seen: Instant::now(),
        },
    );
    server
}
#[tokio::test]
async fn interpreter_snapshot_requires_fresh_private_registration_and_valid_grant_and_service_metadata()
 {
    for problem in [
        "legacy",
        "nil-session",
        "stale",
        "offer",
        "limits",
        "timing",
        "service",
        "identity",
    ] {
        let server = fixture().await;
        assert!(server.interpreter_session("owner", 30).await.is_ok());
        {
            let mut connections = server.inner.connections.write().await;
            let c = connections.get_mut("owner").unwrap();
            match problem {
                "legacy" => c.credential_transport = false,
                "nil-session" => c.session_id = Uuid::nil(),
                "stale" => c.last_seen = Instant::now() - Duration::from_secs(30),
                "offer" => {
                    c.state.as_mut().unwrap().registration["registration_receipt"]["runtime_id"] =
                        json!(Uuid::nil().to_string())
                }
                "limits" => {
                    c.state.as_mut().unwrap().registration["execution_grants"]["limits"]["max_duration_ms"] =
                        json!(0)
                }
                "timing" => {
                    c.state.as_mut().unwrap().registration["execution_grants"]["timing_mode"] =
                        json!("wall_clock")
                }
                "service" => {
                    c.state.as_mut().unwrap().registration["interpreter_service"]["schema_version"] =
                        json!(2)
                }
                "identity" => c.advertisement.as_mut().unwrap().node_id = "other".into(),
                _ => unreachable!(),
            }
        }
        assert!(
            server.interpreter_session("owner", 30).await.is_err(),
            "problem={problem}"
        );
    }
    let server = fixture().await;
    assert!(server.interpreter_session("owner", 0).await.is_err());
    assert!(server.interpreter_session("owner", 301).await.is_err());
    assert!(server.interpreter_session("absent", 30).await.is_err());
}
#[tokio::test]
async fn interpreter_snapshots_pin_runtime_session_and_permission_metadata_without_creating_authority()
 {
    let server = fixture().await;
    let old = server.interpreter_session("owner", 30).await.unwrap();
    assert_eq!(old.node_id(), "owner");
    assert_eq!(old.permission_revision(), Some(3));
    assert!(old.same_connection(&server.interpreter_session("owner", 30).await.unwrap()));
    {
        let mut connections = server.inner.connections.write().await;
        let c = connections.get_mut("owner").unwrap();
        c.session_id = Uuid::new_v4();
        c.state.as_mut().unwrap().registration["registration_receipt"] =
            json!(crate::registration::Offer::new(&Uuid::new_v4().to_string()).unwrap());
        c.state
            .as_mut()
            .unwrap()
            .registration
            .as_object_mut()
            .unwrap()
            .remove("interpreter_service");
    }
    let current = server.interpreter_session("owner", 30).await.unwrap();
    assert!(!old.same_connection(&current));
    assert_ne!(old.runtime_id(), current.runtime_id());
    assert_eq!(
        current.permission_revision(),
        None,
        "Old nodes remain connected but advertise no opt-in service"
    );
    assert_eq!(
        old.permission_revision(),
        Some(3),
        "A retained observation is not silently renewed"
    );
    assert!(!current.serves("ollama", "chosen"));
    assert!(server.inner.pending.lock().await.is_empty());
}

#[tokio::test]
async fn paired_interpreter_checkpoint_pins_registrations_and_never_calls_back_for_invalid_peers() {
    let server = fixture().await;
    let before = server.interpreter_session("owner", 30).await.unwrap();
    server
        .with_interpreter_sessions("owner", "owner", 30, |origin, provider| {
            assert!(origin.same_connection(&before) && provider.same_connection(&before));
            assert!(
                server.inner.connections.try_write().is_err(),
                "Registrations must remain pinned through the callback"
            );
            Ok(())
        })
        .await
        .unwrap();
    assert!(server.inner.connections.try_write().is_ok());
    for (origin, provider, freshness) in [
        ("absent", "owner", 30),
        ("owner", "absent", 30),
        ("owner", "owner", 0),
    ] {
        assert!(
            server
                .with_interpreter_sessions(origin, provider, freshness, |_, _| -> Result<()> {
                    panic!("Invalid peer must not enter the checkpoint")
                })
                .await
                .is_err()
        );
    }
    let error = server
        .with_interpreter_sessions("owner", "owner", 30, |_, _| -> Result<()> {
            anyhow::bail!("Owner checkpoint rejected")
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Owner checkpoint rejected");
    assert!(server.inner.connections.try_write().is_ok());
}
