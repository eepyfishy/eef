use super::*;
use crate::authentication::{AuthMode, AuthenticationPolicy};
use scope::{Limits, Party, Role, Scope};
use std::{collections::BTreeMap, time::Duration};

pub(super) struct Fixture {
    pub scope: Scope,
    pub policy: AuthenticationPolicy,
    pub pair: SealedPair,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let source = "peer-source";
        let destination = "peer-destination";
        let party = |node: &str| Party {
            node_id: node.into(),
            control_session_id: uuid::Uuid::new_v4().to_string(),
            runtime_generation: uuid::Uuid::new_v4().to_string(),
            grant_id: uuid::Uuid::new_v4().to_string(),
            grant_sha256: "b".repeat(64),
            resource_id: crate::context::resource_id(node, "transport"),
        };
        let now = crate::protocol::now_ms();
        let scope = Scope {
            schema_version: 1,
            session_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: identity.fingerprint().unwrap(),
            coordinator_runtime_id: uuid::Uuid::new_v4().to_string(),
            job_id: uuid::Uuid::new_v4().simple().to_string(),
            plan_sha256: "a".repeat(64),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                source.into(),
                vec!["Room".into()],
            )
            .unwrap(),
            source: party(source),
            destination: party(destination),
            destination_address: "127.0.0.1:12345".parse().unwrap(),
            issued_at_ms: now - 1000,
            expires_at_ms: now + 29000,
            limits: Limits {
                max_wire_bytes: 1024 * 1024,
                max_bytes_per_second: 1024 * 1024,
                max_frame_bytes: 4096,
                max_frames: 128,
                max_inflight_frames: 2,
            },
        };
        let policy = AuthenticationPolicy {
            allow_legacy: true,
            credentials: BTreeMap::from([
                (
                    source.into(),
                    Some(crate::authentication::generate_credential()),
                ),
                (
                    destination.into(),
                    Some(crate::authentication::generate_credential()),
                ),
            ]),
            tls: Some(identity),
        };
        let pair = seal_pair(
            &scope,
            &policy,
            "unused-legacy-key",
            &scope.coordinator_runtime_id,
        )
        .unwrap();
        Self {
            scope,
            policy,
            pair,
        }
    }
    pub(super) fn reseal(&mut self) {
        self.pair = seal_pair(
            &self.scope,
            &self.policy,
            "unused-legacy-key",
            &self.scope.coordinator_runtime_id,
        )
        .unwrap();
    }
    pub(super) fn recipient(&self, role: Role) -> Recipient<'_> {
        let party = self.scope.party(role);
        Recipient {
            auth_mode: AuthMode::NodeCredential,
            node_id: &party.node_id,
            control_session_id: &party.control_session_id,
            runtime_generation: &party.runtime_generation,
            coordinator_runtime_id: &self.scope.coordinator_runtime_id,
            coordinator_certificate: &self.policy.tls.as_ref().unwrap().certificate,
            credential: self.policy.credentials[&party.node_id].as_deref().unwrap(),
        }
    }
    pub(super) fn endpoint(&self, role: Role) -> PeerEndpoint {
        open(
            &self.scope,
            if role == Role::Source {
                &self.pair.source
            } else {
                &self.pair.destination
            },
            self.recipient(role),
        )
        .unwrap()
    }
    pub(super) async fn channels(&self) -> (PeerChannel, PeerChannel) {
        let (a, b) = tokio::io::duplex(1024);
        let (a, b) = tokio::join!(
            self.endpoint(Role::Source).connect(a),
            self.endpoint(Role::Destination).accept(b)
        );
        (a.unwrap(), b.unwrap())
    }
}

#[test]
fn sealed_material_is_recipient_scoped_and_never_exports_private_keys() {
    let f = Fixture::new();
    let text = serde_json::to_string(&f.pair).unwrap();
    for secret in f.policy.credentials.values().flatten() {
        assert!(!text.contains(secret));
    }
    for forbidden in ["private_key", "wire_key", "root_certificate"] {
        assert!(!text.contains(forbidden));
    }
    assert!(!text.contains(&f.policy.tls.as_ref().unwrap().private_key));
    let mut recipient = f.recipient(Role::Destination);
    recipient.credential = f.recipient(Role::Source).credential;
    assert!(open(&f.scope, &f.pair.destination, recipient).is_err());
    let mut recipient = f.recipient(Role::Source);
    recipient.auth_mode = AuthMode::Legacy;
    assert!(open(&f.scope, &f.pair.source, recipient).is_err());
    assert!(open(&f.scope, &f.pair.source, f.recipient(Role::Destination)).is_err());
    let debug = format!("{:?}", f.endpoint(Role::Source));
    assert!(!debug.contains("private_key"));
    assert!(!debug.contains(f.recipient(Role::Source).credential));
}

#[test]
fn scope_substitution_expiry_and_old_runtime_material_are_rejected() {
    let f = Fixture::new();
    let changes: [fn(&mut Scope); 9] = [
        |s| s.session_id = uuid::Uuid::new_v4().to_string(),
        |s| s.job_id = uuid::Uuid::new_v4().simple().to_string(),
        |s| s.plan_sha256 = "b".repeat(64),
        |s| s.source.grant_id = uuid::Uuid::new_v4().to_string(),
        |s| s.source.resource_id = crate::context::resource_id(&s.source.node_id, "other"),
        |s| s.destination_address = "127.0.0.1:12346".parse().unwrap(),
        |s| s.limits.max_wire_bytes -= 1,
        |s| s.source.runtime_generation = uuid::Uuid::new_v4().to_string(),
        |s| s.coordinator_runtime_id = uuid::Uuid::new_v4().to_string(),
    ];
    for change in changes {
        let mut scope = f.scope.clone();
        change(&mut scope);
        assert!(open(&scope, &f.pair.source, f.recipient(Role::Source)).is_err());
    }
    let mut recipient = f.recipient(Role::Source);
    recipient.runtime_generation = &f.scope.destination.runtime_generation;
    assert!(open(&f.scope, &f.pair.source, recipient).is_err());
    let mut expired = f.scope.clone();
    expired.expires_at_ms = crate::protocol::now_ms() - 1;
    expired.issued_at_ms = expired.expires_at_ms - 1000;
    assert!(open(&expired, &f.pair.source, f.recipient(Role::Source)).is_err());
    let mut changed = f.pair.source.clone();
    changed.signature = "A".repeat(257);
    assert!(open(&f.scope, &changed, f.recipient(Role::Source)).is_err());
    changed = f.pair.source.clone();
    changed.token = "A".repeat(32769);
    assert!(open(&f.scope, &changed, f.recipient(Role::Source)).is_err());
}

#[test]
fn issuing_requires_current_pinned_independent_unrevoked_credentials() {
    let f = Fixture::new();
    let issue = |policy: &AuthenticationPolicy, legacy: &str, runtime: &str| {
        seal_pair(&f.scope, policy, legacy, runtime)
    };
    let mut policy = f.policy.clone();
    policy
        .credentials
        .insert(f.scope.destination.node_id.clone(), None);
    assert!(issue(&policy, "unused", &f.scope.coordinator_runtime_id).is_err());
    policy = f.policy.clone();
    policy.credentials.insert(
        f.scope.destination.node_id.clone(),
        policy.credentials[&f.scope.source.node_id].clone(),
    );
    assert!(issue(&policy, "unused", &f.scope.coordinator_runtime_id).is_err());
    assert!(
        issue(
            &f.policy,
            f.recipient(Role::Source).credential,
            &f.scope.coordinator_runtime_id
        )
        .is_err()
    );
    assert!(issue(&f.policy, "unused", &uuid::Uuid::new_v4().to_string()).is_err());
    policy = f.policy.clone();
    policy.tls = Some(crate::tls::CoordinatorIdentity::generate().unwrap());
    assert!(issue(&policy, "unused", &f.scope.coordinator_runtime_id).is_err());
}

#[test]
fn scope_bounds_and_resource_ownership_are_intrinsic_not_authority() {
    let f = Fixture::new();
    for change in [
        |s: &mut Scope| s.source.resource_id = "other::transport".into(),
        |s: &mut Scope| s.destination.node_id = s.source.node_id.clone(),
        |s: &mut Scope| s.destination_address = "0.0.0.0:12".parse().unwrap(),
        |s: &mut Scope| s.destination_address = "224.0.0.1:12".parse().unwrap(),
        |s: &mut Scope| s.destination_address = "127.0.0.1:0".parse().unwrap(),
        |s: &mut Scope| s.limits.max_inflight_frames = 9,
        |s: &mut Scope| s.limits.max_wire_bytes = 64 * 1024 * 1024 + 1,
        |s: &mut Scope| s.expires_at_ms = s.issued_at_ms + crate::grants::MAX_DURATION_MS + 1,
        |s: &mut Scope| s.session_id = uuid::Uuid::nil().to_string(),
        |s: &mut Scope| s.limits.max_frames = 0,
    ] {
        let mut scope = f.scope.clone();
        change(&mut scope);
        assert!(scope.validate().is_err());
    }
    let mut raw = serde_json::to_value(&f.scope).unwrap();
    raw["execution_authorized"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Scope>(raw).is_err());
}

#[tokio::test]
async fn real_loopback_tcp_moves_only_framed_authenticated_bounded_data() {
    let mut f = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.scope.destination_address = listener.local_addr().unwrap();
    f.reseal();
    let destination = f.endpoint(Role::Destination);
    let receiver = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut peer = destination.accept(tcp).await.unwrap();
        assert!(peer.send_data(b"wrong direction").await.is_err());
        assert_eq!(
            peer.receive_data().await.unwrap(),
            b"synthetic private payload"
        );
        peer.acknowledge().await.unwrap();
    });
    let tcp = tokio::net::TcpStream::connect(f.scope.destination_address)
        .await
        .unwrap();
    let mut source = f.endpoint(Role::Source).connect(tcp).await.unwrap();
    assert!(source.receive_data().await.is_err());
    source
        .send_data(b"synthetic private payload")
        .await
        .unwrap();
    let ack = source.receive_acknowledgement().await.unwrap();
    assert_eq!(ack.received_frames, 1);
    assert_eq!(ack.received_payload_bytes, 25);
    receiver.await.unwrap();
}

#[tokio::test]
async fn bounded_windows_and_exact_ack_prefixes_allow_no_unbounded_queue() {
    let f = Fixture::new();
    let (mut source, mut destination) = f.channels().await;
    source.send_data(b"one").await.unwrap();
    source.send_data(b"two").await.unwrap();
    assert!(source.send_data(b"full").await.is_err());
    assert_eq!(destination.receive_data().await.unwrap(), b"one");
    assert_eq!(destination.receive_data().await.unwrap(), b"two");
    assert!(destination.receive_data().await.is_err());
    destination.acknowledge().await.unwrap();
    assert!(destination.acknowledge().await.is_err());
    let ack = source.receive_acknowledgement().await.unwrap();
    assert_eq!(ack.received_frames, 2);
    assert_eq!(ack.received_payload_bytes, 6);
    source.send_data(b"three").await.unwrap();
    assert_eq!(destination.receive_data().await.unwrap(), b"three");
    destination.acknowledge().await.unwrap();
    assert_eq!(
        source
            .receive_acknowledgement()
            .await
            .unwrap()
            .received_payload_bytes,
        11
    );
}

#[tokio::test]
async fn separate_scopes_and_roles_do_not_share_tls_authority() {
    let f = Fixture::new();
    let other = Fixture::new();
    let (a, b) = tokio::io::duplex(1024);
    let (source, destination) = tokio::join!(
        f.endpoint(Role::Source).connect(a),
        other.endpoint(Role::Destination).accept(b)
    );
    assert!(source.is_err());
    assert!(destination.is_err());
    let (a, b) = tokio::io::duplex(1024);
    assert!(f.endpoint(Role::Destination).connect(a).await.is_err());
    drop(b);
}

#[tokio::test]
async fn frame_count_size_and_wire_budget_fail_without_claiming_delivery() {
    let mut f = Fixture::new();
    f.scope.limits.max_frames = 1;
    f.reseal();
    let (mut source, mut destination) = f.channels().await;
    assert!(source.send_data(&vec![0; 4097]).await.is_err());
    source.send_data(b"one").await.unwrap();
    destination.receive_data().await.unwrap();
    destination.acknowledge().await.unwrap();
    source.receive_acknowledgement().await.unwrap();
    assert!(source.send_data(b"two").await.is_err());
    assert!(destination.receive_data().await.is_err());
    let mut f = Fixture::new();
    f.scope.limits.max_wire_bytes = 32;
    f.reseal();
    let (mut source, mut destination) = f.channels().await;
    assert!(source.send_data(b"no frame fits").await.is_err());
    assert!(source.send_data(b"again").await.is_err());
    drop(source);
    assert!(destination.receive_data().await.is_err());
}

#[tokio::test]
async fn per_channel_rate_budget_paces_application_bytes_without_unbounded_buffering() {
    let mut f = Fixture::new();
    f.scope.limits.max_bytes_per_second = 300;
    f.reseal();
    let (mut source, mut destination) = f.channels().await;
    let receiver = tokio::spawn(async move {
        let bytes = destination.receive_data().await.unwrap();
        assert_eq!(bytes, vec![7; 64]);
        destination.acknowledge().await.unwrap();
    });
    let start = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(10), async {
        source.send_data(&[7; 64]).await.unwrap();
        source.receive_acknowledgement().await.unwrap();
        receiver.await.unwrap();
    })
    .await
    .unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(200),
        "declared low rate must not be ignored"
    );
}

#[tokio::test]
async fn a_waiting_receive_wakes_at_lease_expiry_even_without_network_activity() {
    let mut f = Fixture::new();
    f.scope.expires_at_ms = crate::protocol::now_ms() + 3000;
    f.reseal();
    let (_source, mut destination) = f.channels().await;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), destination.receive_data())
            .await
            .unwrap()
            .is_err()
    );
    assert!(destination.receive_data().await.is_err());
}
