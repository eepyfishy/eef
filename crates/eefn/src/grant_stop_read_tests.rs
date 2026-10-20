use super::*;
struct Fixture {
    _dir: tempfile::TempDir,
    config: std::path::PathBuf,
    path: std::path::PathBuf,
    ledger: Arc<GrantLedger>,
    grant: ExecutionGrant,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        crate::setup::write_json(
            &config,
            &json!({"node_id":"owner","psk":"private-fixture","endpoints":[]}),
        )
        .unwrap();
        let path = crate::grants::journal_path(&config);
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        Self {
            _dir: dir,
            config,
            path,
            ledger,
            grant: crate::grants::tests::grant(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::from_grant(&self.grant).unwrap()
    }
    fn inspect(&self) -> Inspection {
        self.ledger
            .inspect_stop(&self.scope(), &self.grant.issuer_id, "owner")
            .unwrap()
    }
    fn stop(&self) -> Report {
        self.ledger
            .request_stop(
                &self.grant,
                &self.grant.issuer_id,
                &self.grant.session_id,
                "owner",
            )
            .unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.path).unwrap()
    }
}
#[test]
fn stop_inspection_missing_is_read_only_without_schema_creation_or_signal() {
    let f = Fixture::new();
    let before = f.bytes();
    let missing = f.inspect();
    assert_eq!(missing.availability, Availability::Missing);
    assert!(missing.confirms(&f.scope()));
    assert!(missing.evidence.is_none());
    assert_eq!(f.bytes(), before);
    assert_eq!(
        f.ledger
            .connection
            .lock()
            .unwrap()
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_master WHERE name='execution_stop_requests'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        0
    );
    f.ledger.claim(&f.grant).unwrap();
    let watcher = f.ledger.watch_stop(&f.grant).unwrap();
    let before = f.bytes();
    assert_eq!(f.inspect().availability, Availability::Missing);
    assert!(!*watcher.receiver.borrow());
    assert!(!watcher.receiver.has_changed().unwrap());
    assert_eq!(f.bytes(), before);
}
#[test]
fn stop_inspection_retained_is_byte_stable_without_repeated_notifications_or_invented_signal_history()
 {
    let f = Fixture::new();
    f.ledger.claim(&f.grant).unwrap();
    let mut watcher = f.ledger.watch_stop(&f.grant).unwrap();
    let live = f.stop();
    assert!(live.cancellation_signalled);
    assert!(*watcher.receiver.borrow_and_update());
    let before = f.bytes();
    let value = f.inspect();
    let retained = value.evidence.as_ref().unwrap();
    assert_eq!(retained.recorded_at_ms, live.recorded_at_ms);
    assert!(retained.persisted);
    assert!(!retained.execution_stopped);
    assert!(!retained.external_effects_stopped);
    assert!(json!(retained).get("cancellation_signalled").is_none());
    assert!(value.read_only);
    assert!(!value.stop_requested);
    assert!(!value.execution_requested);
    assert_eq!(f.bytes(), before);
    assert!(!watcher.receiver.has_changed().unwrap());
    let service = crate::NodeService::new(f.config.clone(), "owner".into());
    let local = service
        .local_stop_receipt(crate::grants::ReceiptRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            grant_id: f.grant.grant_id.clone(),
        })
        .unwrap();
    let local: Inspection = serde_json::from_value(local).unwrap();
    assert_eq!(local, value);
    assert_eq!(f.bytes(), before);
    assert!(!watcher.receiver.has_changed().unwrap());
}
#[test]
fn stop_inspection_history_survives_reopen_without_retagging_the_original_session() {
    let f = Fixture::new();
    f.stop();
    let scope = f.scope();
    let before = f.bytes();
    let reopened = GrantLedger::open(&f.path, Default::default(), "owner").unwrap();
    let history = reopened
        .inspect_stop(&scope, &scope.issuer_id, "owner")
        .unwrap();
    assert_eq!(history.availability, Availability::Retained);
    assert_eq!(f.bytes(), before);
    let wire = WireRequest {
        kind: "inspect_grant_stop".into(),
        id: "a".repeat(32),
        scope: scope.clone(),
    };
    wire.validate(&scope.issuer_id, "owner").unwrap();
    let mut retagged = scope.clone();
    retagged.session_id = uuid::Uuid::new_v4().to_string();
    assert!(
        reopened
            .inspect_stop(&retagged, &scope.issuer_id, "owner")
            .is_err()
    );
    assert!(wire.validate(&"b".repeat(64), "owner").is_err());
    assert!(wire.validate(&scope.issuer_id, "other").is_err());
    assert_eq!(f.bytes(), before);
}
#[test]
fn stop_inspection_corrupt_or_oversized_evidence_is_not_missing_and_is_never_repaired() {
    for scenario in 0..7 {
        let f = Fixture::new();
        f.stop();
        let db = Connection::open(&f.path).unwrap();
        match scenario {
            0 => {
                db.execute_batch("UPDATE execution_stop_meta SET schema_version=99")
                    .unwrap();
            }
            1 => {
                db.execute(
                    "UPDATE execution_stop_requests SET node_id=?1",
                    ["x".repeat(5000)],
                )
                .unwrap();
            }
            2 => {
                db.execute_batch("UPDATE execution_stop_requests SET receipt_preexisted=2")
                    .unwrap();
            }
            3 => {
                db.execute_batch("UPDATE execution_stop_requests SET recorded_ms=0")
                    .unwrap();
            }
            4 => {
                db.execute_batch("DELETE FROM execution_receipts").unwrap();
            }
            5 => {
                db.execute_batch("DROP TABLE execution_stop_meta").unwrap();
            }
            _ => {
                db.execute_batch(
                    "UPDATE execution_stop_requests SET job_id='ffffffffffffffffffffffffffffffff'",
                )
                .unwrap();
            }
        }
        drop(db);
        let before = f.bytes();
        assert!(
            f.ledger
                .inspect_stop(&f.scope(), &f.grant.issuer_id, "owner")
                .is_err(),
            "scenario {scenario}"
        );
        assert_eq!(f.bytes(), before);
    }
}
#[test]
fn stop_inspection_report_rejects_missing_fields_and_forged_actions_or_termination() {
    let f = Fixture::new();
    f.stop();
    let value = json!(f.inspect());
    for key in ["scope", "evidence"] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<Inspection>(bad).is_err());
    }
    for key in [
        "execution_requested",
        "stop_requested",
        "automatically_retried",
        "reconciled",
    ] {
        let mut bad = value.clone();
        bad[key] = json!(true);
        assert!(
            serde_json::from_value::<Inspection>(bad)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut bad = value;
    bad["evidence"]["execution_stopped"] = json!(true);
    assert!(
        serde_json::from_value::<Inspection>(bad)
            .unwrap()
            .validate()
            .is_err()
    );
}
#[test]
fn stop_inspection_local_missing_file_never_initializes_a_journal() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("node.json");
    crate::setup::write_json(&config, &json!({"node_id":"owner","endpoints":[]})).unwrap();
    let service = crate::NodeService::new(config.clone(), "owner".into());
    assert!(
        service
            .local_stop_receipt(crate::grants::ReceiptRequest {
                schema_version: 1,
                expected_node_id: "owner".into(),
                grant_id: uuid::Uuid::new_v4().to_string()
            })
            .is_err()
    );
    assert!(!crate::grants::journal_path(&config).exists());
}
