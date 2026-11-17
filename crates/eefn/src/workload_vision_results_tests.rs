use super::*;
use crate::vision_result_inspection::WireRequest;
use std::time::Duration;

fn definition(calls: u64) -> Definition {
    let mut d = crate::workload_leases::definition::vision::tests::vision();
    if let Source::CameraVision { parameters, .. } = &mut d.source {
        parameters.capture.max_frames = u32::try_from(calls + 1).unwrap();
        parameters.images.max_images = calls;
    }
    d.limits.max_source_frames = calls + 1;
    d.validate().unwrap();
    d
}
async fn saved(path: &Path, d: &Definition, complete: bool) -> Arc<GrantLedger> {
    let ledger = GrantLedger::open(path, Default::default(), &d.executor_node).unwrap();
    let consumed = super::tests::consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            for sequence in 1..=parameters(d)?.max_calls() {
                let permit = writer
                    .begin_analysis(
                        sequence + 1,
                        &super::tests::image(),
                        Arc::new(reporting.checkpoint(&contract)?),
                    )
                    .await?;
                if complete {
                    writer
                        .finish_analysis(
                            permit,
                            format!("result {sequence}"),
                            Arc::new(reporting.checkpoint(&contract)?),
                        )
                        .await?;
                }
            }
            Ok(())
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state
            != "unknown"
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    ledger
}
fn query(d: &Definition) -> WireRequest {
    WireRequest {
        kind: "workload_analyses".into(),
        schema_version: 1,
        id: uuid::Uuid::new_v4().simple().to_string(),
        scope: d.scope().unwrap(),
        after_sequence: 0,
        limit: MAX_PAGE,
        expected_prefix_sha256: Page::seed(d).unwrap(),
    }
}

#[tokio::test]
async fn retained_inspection_rejects_cross_page_order_even_with_valid_page_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let d = definition(20);
    let ledger = saved(&dir.path().join("journal.db"), &d, true).await;
    let seed = Page::seed(&d).unwrap();
    let mut first = ledger
        .workload_analysis_page(&d.issuer_id, &d, 0, MAX_PAGE, &seed)
        .unwrap();
    let mut second = ledger
        .workload_analysis_page(&d.issuer_id, &d, 16, MAX_PAGE, &first.prefix_after_sha256)
        .unwrap();
    // Both pages remain internally ordered and checksummed. Only the joined
    // inspection can detect reuse of the preceding page's source observation.
    let row = &mut second.analyses[0];
    row.intent.source_sequence = first.analyses[15].intent.source_sequence;
    row.intent_sha256 = hash(&serde_json::to_vec(&row.intent).unwrap());
    let result = row.result.as_mut().unwrap();
    result.intent_sha256 = row.intent_sha256.clone();
    row.result_sha256 = Some(hash(&serde_json::to_vec(result).unwrap()));
    let prefix = second
        .analyses
        .iter()
        .fold(first.prefix_after_sha256.clone(), |p, r| r.next_prefix(&p));
    first.summary.prefix_sha256 = prefix.clone();
    second.summary.prefix_sha256 = prefix.clone();
    second.prefix_after_sha256 = prefix;
    first.validate(&d, 0, MAX_PAGE, &seed).unwrap();
    second
        .validate(&d, 16, MAX_PAGE, &first.prefix_after_sha256)
        .unwrap();
    assert!(Inspection::from_pages(&d, vec![first, second]).is_err());
}

#[tokio::test]
async fn empty_retained_inspection_keeps_zero_evidence_and_never_creates_completion() {
    let dir = tempfile::tempdir().unwrap();
    let d = definition(1);
    let ledger = GrantLedger::open(
        &dir.path().join("journal.db"),
        Default::default(),
        &d.executor_node,
    )
    .unwrap();
    let consumed = super::tests::consume(&ledger, d.clone()).await.unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async { Ok(()) })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state
            != "unknown"
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let page = query(&d)
        .read(&ledger, &d.issuer_id, &d.executor_node)
        .unwrap();
    let inspection = Inspection::from_pages(&d, vec![page.clone()]).unwrap();
    assert_eq!(inspection.summary.calls, 0);
    assert_eq!(inspection.summary.result_bytes, 0);
    assert_eq!(inspection.summary.prefix_sha256, Page::seed(&d).unwrap());
    assert!(inspection.analyses.is_empty() && inspection.completion.is_none());
    assert!(History::from_pages(&d, vec![page]).is_err());
    let mut changed = inspection;
    changed.summary.last_source_sequence = 2;
    assert!(changed.validate(&d).is_err());
    assert!(super::tests::consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn paged_retained_results_chain_exact_prefixes_without_turning_unknown_work_into_completion()
{
    let dir = tempfile::tempdir().unwrap();
    let d = definition(20);
    let ledger = saved(&dir.path().join("journal.db"), &d, true).await;
    let seed = Page::seed(&d).unwrap();
    let first = ledger
        .workload_analysis_page(&d.issuer_id, &d, 0, MAX_PAGE, &seed)
        .unwrap();
    assert_eq!(first.analyses.len(), 16);
    assert!(first.more);
    assert_eq!(first.receipt.state, "unknown");
    assert_eq!(first.analyses[0].text(), Some("result 1"));
    assert!(
        !format!("{first:?}").contains("result 1"),
        "Diagnostic formatting must not expose retained text"
    );
    assert!(first.completion.is_none());
    let second = ledger
        .workload_analysis_page(
            &d.issuer_id,
            &d,
            first.next_sequence,
            MAX_PAGE,
            &first.prefix_after_sha256,
        )
        .unwrap();
    assert_eq!(second.analyses.len(), 4);
    assert!(!second.more);
    assert_eq!(second.analyses[3].text(), Some("result 20"));
    second
        .validate(&d, 16, MAX_PAGE, &first.prefix_after_sha256)
        .unwrap();
    assert_eq!(second.prefix_after_sha256, second.summary.prefix_sha256);
    let inspection = Inspection::from_pages(&d, vec![first.clone(), second.clone()]).unwrap();
    assert_eq!(inspection.receipt.state, "unknown");
    assert_eq!(inspection.analyses.len(), 20);
    assert!(inspection.completion.is_none());
    assert!(History::from_pages(&d, vec![first.clone(), second]).is_err());
    assert!(
        ledger
            .workload_analysis_page(&d.issuer_id, &d, 16, MAX_PAGE, &seed)
            .is_err()
    );
    for limit in [0, MAX_PAGE + 1, usize::MAX] {
        assert!(
            ledger
                .workload_analysis_page(&d.issuer_id, &d, 0, limit, &seed)
                .is_err()
        );
    }
    assert!(
        ledger
            .workload_analysis_page(&d.issuer_id, &d, 21, 1, &seed)
            .is_err()
    );
    let raw = serde_json::to_string(&first).unwrap();
    assert!(
        !raw.contains("PRIVATE_CAMERA") && !raw.contains("image_base64") && !raw.contains("prompt")
    );
}

#[tokio::test]
async fn completed_value_assembly_checks_multiple_pages_without_minting_source_authority() {
    let dir = tempfile::tempdir().unwrap();
    let d = definition(20);
    let ledger = saved(&dir.path().join("journal.db"), &d, true).await;
    let first = ledger
        .workload_analysis_page(&d.issuer_id, &d, 0, MAX_PAGE, &Page::seed(&d).unwrap())
        .unwrap();
    let second = ledger
        .workload_analysis_page(&d.issuer_id, &d, 16, MAX_PAGE, &first.prefix_after_sha256)
        .unwrap();
    // Deliberately fabricated report metadata tests value validation only. It
    // cannot enter the journal's opaque observed-source completion constructor.
    let report = super::super::CompletionReport {
        schema_version: 3,
        grant_sha256: d.scope().unwrap().grant_sha256,
        adapter_sha256: d.source.adapter_sha256().into(),
        source_observations: d.source.max_observations(),
        source_frames: d.limits.max_source_frames,
        event_count: 1,
        event_bytes: 128,
        event_prefix_sha256: "a".repeat(64),
        event_payload_prefix_sha256: Some("b".repeat(64)),
        analysis: Some(first.summary.clone()),
        lease_sequence: first.summary.last_lease_sequence,
        recorded_at_ms: first.receipt.updated_at_ms,
        owned_process_exit_observed: true,
        physical_effects_stopped: false,
    };
    let mut pages = vec![first, second];
    for page in &mut pages {
        page.receipt.state = "completed".into();
        page.completion = Some(report.clone());
    }
    let history = History::from_pages(&d, pages.clone()).unwrap();
    history.validate(&d).unwrap();
    assert_eq!(history.analyses.len(), 20);
    assert_eq!(history.analyses[19].text(), Some("result 20"));
    let mut reordered = pages.clone();
    reordered.swap(0, 1);
    assert!(History::from_pages(&d, reordered).is_err());
    let mut changed = pages.clone();
    changed[1].receipt.updated_at_ms += 1;
    assert!(History::from_pages(&d, changed).is_err());
    let mut changed = history.clone();
    changed.analyses.pop();
    assert!(changed.validate(&d).is_err());
    // Structured values and reads leave the actual uncertain receipt unchanged.
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}

#[tokio::test]
async fn result_page_validation_refuses_scope_cursor_record_and_summary_substitution() {
    let dir = tempfile::tempdir().unwrap();
    let d = definition(1);
    let ledger = saved(&dir.path().join("journal.db"), &d, true).await;
    let seed = Page::seed(&d).unwrap();
    let page = ledger
        .workload_analysis_page(&d.issuer_id, &d, 0, MAX_PAGE, &seed)
        .unwrap();
    for (pointer, value) in [
        ("/grant_sha256", json!("b".repeat(64))),
        ("/schema_version", json!(1)),
        ("/after_sequence", json!(1)),
        ("/next_sequence", json!(0)),
        ("/more", json!(true)),
        ("/execution_authorized", json!(true)),
        ("/restart_authorized", json!(true)),
        ("/model_replay_authorized", json!(true)),
        ("/prefix_before_sha256", json!("a".repeat(64))),
        ("/prefix_after_sha256", json!("b".repeat(64))),
        ("/summary/prefix_sha256", json!("c".repeat(64))),
        ("/summary/calls", json!(2)),
        ("/analyses/0/intent/source_sequence", json!(1)),
        ("/analyses/0/result/text", json!("changed")),
        ("/analyses/0/intent_sha256", json!("d".repeat(64))),
        ("/receipt/task_id", json!("other-task")),
    ] {
        let mut value_page = json!(page);
        *value_page.pointer_mut(pointer).unwrap() = value;
        let claimed: Page = serde_json::from_value(value_page).unwrap();
        assert!(
            claimed.validate(&d, 0, MAX_PAGE, &seed).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = json!(page);
    unknown["analyses"][0]["image_base64"] = json!("PRIVATE_IMAGE");
    assert!(serde_json::from_value::<Page>(unknown).is_err());
    assert!(
        ledger
            .workload_analysis_page(&"b".repeat(64), &d, 0, MAX_PAGE, &seed)
            .is_err()
    );
    let mut other = d.clone();
    other.plan_sha256 = if d.plan_sha256 == "b".repeat(64) {
        "c".repeat(64)
    } else {
        "b".repeat(64)
    };
    assert!(
        ledger
            .workload_analysis_page(
                &d.issuer_id,
                &other,
                0,
                MAX_PAGE,
                &Page::seed(&other).unwrap()
            )
            .is_err()
    );
}

#[tokio::test]
async fn pending_analysis_read_is_explicitly_uncertain_and_cannot_supply_a_completed_history() {
    let dir = tempfile::tempdir().unwrap();
    let d = definition(1);
    let ledger = saved(&dir.path().join("journal.db"), &d, false).await;
    let page = query(&d)
        .read(&ledger, &d.issuer_id, &d.executor_node)
        .unwrap();
    assert_eq!(page.receipt.state, "unknown");
    assert_eq!(page.summary.calls, 1);
    assert_eq!(page.summary.completed, 0);
    assert_eq!(page.analyses[0].text(), None);
    assert!(page.completion.is_none());
    let inspection = Inspection::from_pages(&d, vec![page.clone()]).unwrap();
    assert_eq!(inspection.summary.completed, 0);
    assert_eq!(inspection.analyses[0].text(), None);
    let mut changed = inspection.clone();
    changed.summary.completed = 1;
    assert!(changed.validate(&d).is_err());
    let mut changed = inspection.clone();
    changed.receipt.state = "completed".into();
    assert!(changed.validate(&d).is_err());
    let mut changed = json!(inspection);
    changed["execution_authorized"] = json!(true);
    assert!(serde_json::from_value::<Inspection>(changed).is_err());
    assert!(History::from_pages(&d, vec![page]).is_err());
    assert!(super::tests::consume(&ledger, d).await.is_err());
}

#[test]
fn analysis_read_requires_distinct_protocol_original_issuer_and_bounded_correlated_scope() {
    let d = definition(1);
    let q = query(&d);
    q.validate(&d.issuer_id, &d.executor_node).unwrap();
    assert!(q.validate(&"b".repeat(64), &d.executor_node).is_err());
    assert!(q.validate(&d.issuer_id, "different-node").is_err());
    for (field, value) in [
        ("type", json!("workload_receipt")),
        ("schema_version", json!(2)),
        ("limit", json!(0)),
        ("limit", json!(MAX_PAGE + 1)),
        ("after_sequence", json!(101)),
        ("expected_prefix_sha256", json!("f".repeat(64))),
        ("id", json!("")),
    ] {
        let mut altered = json!(q);
        altered[field] = value;
        let altered: WireRequest = serde_json::from_value(altered).unwrap();
        assert!(
            altered.validate(&d.issuer_id, &d.executor_node).is_err(),
            "{field}"
        );
    }
    let old: crate::workload_inspection::WireRequest =
        serde_json::from_value(json!({"type":"workload_receipt",
        "id":q.id,"scope":q.scope}))
        .unwrap();
    old.validate(&d.issuer_id, &d.executor_node).unwrap();
    assert!(
        serde_json::from_value::<WireRequest>(
            json!({"type":"workload_analyses","id":q.id,"scope":q.scope})
        )
        .is_err()
    );
}

#[tokio::test]
async fn authenticated_result_read_survives_reconnect_without_source_permission_or_replay() {
    let dir = tempfile::tempdir().unwrap();
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    let mut d = definition(1);
    d.issuer_id = identity.fingerprint().unwrap();
    let path = dir.path().join("journal.db");
    let ledger = saved(&path, &d, true).await;
    let credential = crate::authentication::generate_credential();
    let server = crate::NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            tls: Some(identity.clone()),
            credentials: std::collections::BTreeMap::from([(
                d.executor_node.clone(),
                Some(credential.clone()),
            )]),
        },
    )
    .unwrap();
    server.start().await.unwrap();
    let config=serde_json::from_value(json!({"node_id":d.executor_node,"psk":credential,"auth_mode":"node_credential",
        "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",server.port())],"heartbeat_seconds":0.2})).unwrap();
    let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let runtime = uuid::Uuid::new_v4().to_string();
    let status = Arc::new(std::sync::Mutex::new(json!({"runtime_id":runtime})));
    let client = crate::NodeClient::new(config, engine)
        .unwrap()
        .with_grants(&path, Default::default())
        .with_registration_runtime(&runtime)
        .unwrap()
        .with_status(status.clone());
    assert_eq!(
        status.lock().unwrap()["execution_grants"]["available"],
        true,
        "Fixture requires its actual reopened journal"
    );
    let mut running = tokio::spawn(async move { client.connect_once().await });
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        while server.node_state(&d.executor_node).await.is_none()
            || server.grant_session(&d.executor_node).await.is_err()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    tokio::select! {
        result=&mut running=>panic!("Retained-read fixture exited before registration: {result:?}"),
        result=ready=>result.expect("Retained-read fixture did not register"),
    }
    let before = ledger
        .workload_analyses(&d.issuer_id, &d.grant_id, 0, 1)
        .unwrap();
    let page = server
        .inspect_workload_analyses(&d, 0, MAX_PAGE, &Page::seed(&d).unwrap())
        .await
        .unwrap();
    assert_eq!(page.receipt.state, "unknown");
    assert_eq!(page.analyses[0].text(), Some("result 1"));
    let inspection = server
        .retained_workload_analyses(&d, tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(inspection.receipt.state, "unknown");
    assert_eq!(inspection.analyses[0].text(), Some("result 1"));
    assert!(
        server
            .completed_workload_analyses(&d, tokio::time::Instant::now() + Duration::from_secs(5),)
            .await
            .is_err()
    );
    assert!(
        !page.execution_authorized && !page.restart_authorized && !page.model_replay_authorized
    );
    let mut wrong = d.clone();
    wrong.issuer_id = "b".repeat(64);
    assert!(
        server
            .inspect_workload_analyses(&wrong, 0, MAX_PAGE, &Page::seed(&wrong).unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        ledger
            .workload_analyses(&d.issuer_id, &d.grant_id, 0, 1)
            .unwrap(),
        before
    );
    server.stop().await;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(
        server
            .inspect_workload_analyses(&d, 0, MAX_PAGE, &Page::seed(&d).unwrap())
            .await
            .is_err()
    );
}
