use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Fixture {
    service: Arc<NodeService>,
    runtime: String,
    pulls: Arc<AtomicUsize>,
    hold_pull: Arc<AtomicBool>,
    pull_gate: Arc<tokio::sync::Notify>,
    hold_tags: Arc<AtomicBool>,
    tags_started: Arc<AtomicUsize>,
    tags_gate: Arc<tokio::sync::Notify>,
    server: tokio::task::JoinHandle<()>,
    // Drop runtime/database owners before temporary-directory cleanup on Windows.
    dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.pull_gate.notify_waiters();
        self.tags_gate.notify_waiters();
        self.server.abort();
    }
}
impl Fixture {
    async fn new(installed: bool) -> Self {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let installed = Arc::new(AtomicBool::new(installed));
        let pulls = Arc::new(AtomicUsize::new(0));
        let present = installed.clone();
        let changed = installed.clone();
        let count = pulls.clone();
        let hold_pull = Arc::new(AtomicBool::new(false));
        let pull_gate = Arc::new(tokio::sync::Notify::new());
        let hold_tags = Arc::new(AtomicBool::new(false));
        let tags_gate = Arc::new(tokio::sync::Notify::new());
        let tags_started = Arc::new(AtomicUsize::new(0));
        let pull_hold = hold_pull.clone();
        let pull_wait = pull_gate.clone();
        let tags_hold = hold_tags.clone();
        let tags_wait = tags_gate.clone();
        let tags_seen = tags_started.clone();
        let app=Router::new()
            .route("/api/tags",get(move||{let present=present.clone();let hold=tags_hold.clone();let wait=tags_wait.clone();let seen=tags_seen.clone();async move{seen.fetch_add(1,Ordering::SeqCst);if hold.load(Ordering::SeqCst){wait.notified().await;}Json(json!({"models":if present.load(Ordering::SeqCst){json!([{"name":"fixture:latest"}])}else{json!([])}}))}}))
            .route("/api/show",post(||async{Json(json!({"capabilities":["completion"]}))}))
            .route("/api/pull",post(move||{let changed=changed.clone();let count=count.clone();let hold=pull_hold.clone();let wait=pull_wait.clone();async move{count.fetch_add(1,Ordering::SeqCst);if hold.load(Ordering::SeqCst){wait.notified().await;}changed.store(true,Ordering::SeqCst);"{\"status\":\"success\"}\n"}}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let mut value = json!({"node_id":"node","psk":"fixture-only","endpoints":[],"future":{"keep":true},
            "models":{"provider":"auto","ollama":{"base_url":format!("http://{address}"),"selected":[]},"llamacpp":{"slots":[]}},
            "model_catalog":[{"id":"fixture","modality":"text","ollama":"fixture:latest","url":"https://example.invalid/weights.gguf?private-source=not-reported",
                "sha256":"a".repeat(64),"bytes":32,"license":"Apache-2.0","source_page":"https://example.invalid/license"}]});
        let pin = CatalogPin::from_config(&value, "fixture").unwrap();
        value["model_bootstrap"] = json!({"mode":"auto","automation":true,"catalog":pin});
        crate::setup::write_json(&path, &value).unwrap();
        let service = NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            runtime,
            pulls,
            hold_pull,
            pull_gate,
            hold_tags,
            tags_started,
            tags_gate,
            server,
        }
    }
    fn record(&self) -> Attempt {
        Journal::new(&self.service).read().unwrap().unwrap()
    }
    fn status(&self) -> Status {
        let c = self.service.read_config().unwrap();
        status(
            &self.service,
            &c,
            &Saved::from_config(&c).unwrap(),
            &self.service.live.lock().unwrap(),
        )
    }
    async fn tick(&self) {
        tick(
            self.service.clone(),
            &self.runtime,
            &Journal::new(&self.service),
        )
        .await
        .unwrap();
    }
}

async fn wait_count(count: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while count.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn setup_revocation_during_async_provider_inspection_prevents_admission() {
    let f = Fixture::new(false).await;
    f.hold_tags.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let runtime = f.runtime.clone();
    let task =
        tokio::spawn(async move { tick(service.clone(), &runtime, &Journal::new(&service)).await });
    wait_count(&f.tags_started).await;
    let mut c = f.service.read_config().unwrap();
    c["model_bootstrap"]["automation"] = json!(false);
    f.service.save_config(c).unwrap();
    f.hold_tags.store(false, Ordering::SeqCst);
    f.tags_gate.notify_one();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(f.record().phase, Phase::Stopped);
    assert_eq!(f.record().reason, Some(Reason::StateChanged));
    assert!(!f.record().download_admitted);
    assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn setup_revocation_cancels_only_its_admitted_transfer_before_selection() {
    let f = Fixture::new(false).await;
    f.hold_pull.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let runtime = f.runtime.clone();
    let task =
        tokio::spawn(async move { tick(service.clone(), &runtime, &Journal::new(&service)).await });
    wait_count(&f.pulls).await;
    let id = f.record().id;
    let mut c = f.service.read_config().unwrap();
    c["model_bootstrap"]["automation"] = json!(false);
    f.service.save_config(c).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(f.record().phase, Phase::Stopped);
    assert_eq!(f.record().id, id);
    assert!(
        crate::model_selection::selection_view(&f.service.read_config().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.service.live.lock().unwrap()["download"]["id"], id);
    assert_eq!(
        f.service.live.lock().unwrap()["download"]["cancel_requested"],
        true
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if f.service
                .download_receipts
                .lock()
                .unwrap()
                .get(&id)
                .unwrap()
                .unwrap()
                .state
                == crate::download_receipts::State::Cancelled
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    f.hold_pull.store(false, Ordering::SeqCst);
    f.pull_gate.notify_one();
}

#[tokio::test]
async fn setup_cpu_gguf_fallback_verifies_existing_cache_and_preserves_bad_bytes() {
    use sha2::{Digest, Sha256};
    for corrupt in [false, true] {
        let f = Fixture::new(false).await;
        let bytes = b"GGUF synthetic metadata fixture, not working inference weights";
        let hash = hex::encode(Sha256::digest(bytes));
        let directory = f.service.model_directory();
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{hash}.gguf"));
        let mut actual = bytes.to_vec();
        if corrupt {
            actual[0] ^= 1;
        }
        std::fs::write(&path, &actual).unwrap();
        let mut c = f.service.read_config().unwrap();
        c["models"]["provider"] = json!("llamacpp");
        c["model_catalog"][0]["sha256"] = json!(hash);
        c["model_catalog"][0]["bytes"] = json!(bytes.len());
        c["model_bootstrap"]["catalog"] = json!(CatalogPin::from_config(&c, "fixture").unwrap());
        f.service
            .inspect_grant_configuration(|_| f.service.save_bootstrap_config_locked(c))
            .unwrap();
        f.tick().await;
        let record = f.record();
        assert_eq!(record.backend, Backend::Llamacpp);
        assert_eq!(std::fs::read(&path).unwrap(), actual);
        assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
        if corrupt {
            assert_eq!(record.phase, Phase::Failed);
            assert!(
                crate::model_selection::selection_view(&f.service.read_config().unwrap())
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(record.phase, Phase::Configured);
            let configured = f.service.read_config().unwrap();
            assert_eq!(configured["models"]["provider"], "llamacpp");
            assert_eq!(
                configured["models"]["llamacpp"]["slots"][0]["gpu_layers"],
                0
            );
            assert_eq!(
                configured["models"]["llamacpp"]["slots"][0]["selection"]["roles"],
                json!(["request_interpreter"])
            );
        }
        let id = record.id;
        f.tick().await;
        assert_eq!(f.record().id, id);
    }
}

#[tokio::test]
async fn setup_existing_ollama_configures_interpreter_without_download_or_restart() {
    let f = Fixture::new(true).await;
    let before = f.service.read_config().unwrap();
    f.tick().await;
    assert_eq!(f.record().phase, Phase::Configured);
    assert!(!f.record().download_admitted);
    assert_eq!(f.record().backend, Backend::Ollama);
    assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
    let saved = f.service.read_config().unwrap();
    assert_eq!(saved["future"], before["future"]);
    assert_eq!(saved["models"]["provider"], "ollama");
    let view = crate::model_selection::selection_view(&saved).unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0]["model_id"], "fixture:latest");
    assert_eq!(
        view[0]["model_metadata"]["roles"],
        json!(["request_interpreter"])
    );
    let shown = f.status();
    shown
        .validate("node", &Saved::from_config(&saved).unwrap())
        .unwrap();
    assert!(shown.restart_required);
    assert!(!shown.readiness_verified);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), f.service.restart.notified())
            .await
            .is_err()
    );
    let id = f.record().id;
    f.tick().await;
    assert_eq!(f.record().id, id);
    let text = serde_json::to_string(&shown).unwrap();
    assert!(!text.contains("private-source"));
    assert!(!text.contains("fixture-only"));
}

#[tokio::test]
async fn setup_missing_ollama_reuses_one_installer_receipt_then_selects() {
    let f = Fixture::new(false).await;
    f.tick().await;
    let record = f.record();
    assert_eq!(record.phase, Phase::Configured);
    assert!(record.download_admitted);
    assert_eq!(f.pulls.load(Ordering::SeqCst), 1);
    let receipt = f
        .service
        .download_receipts
        .lock()
        .unwrap()
        .get(&record.id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state, crate::download_receipts::State::Installed);
    f.tick().await;
    assert_eq!(f.pulls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn setup_legacy_auto_prompt_opt_out_and_existing_models_have_no_side_effects() {
    for case in [
        "legacy",
        "prompt",
        "off",
        "managed",
        "custom",
        "applied_cli",
        "catalog_changed",
        "no_default",
    ] {
        let f = Fixture::new(false).await;
        let mut value = f.service.read_config().unwrap();
        match case {
            "legacy" => {
                value["model_bootstrap"]
                    .as_object_mut()
                    .unwrap()
                    .remove("automation");
            }
            "prompt" | "off" | "managed" => value["model_bootstrap"]["mode"] = json!(case),
            "custom" => {
                value["models"]["ollama"]["selected"] =
                    json!([{"model_id":"custom-owner-model","modality":"text"}])
            }
            "applied_cli" => {
                f.service.live.lock().unwrap()["setup_existing_applied_models"] = json!(true)
            }
            "catalog_changed" => value["model_catalog"][0]["name"] = json!("Changed after pinning"),
            "no_default" => value["model_bootstrap"]["catalog"] = Value::Null,
            _ => unreachable!(),
        }
        if case == "no_default" {
            f.service
                .inspect_grant_configuration(|_| f.service.save_bootstrap_config_locked(value))
                .unwrap();
        } else {
            f.service.save_config(value).unwrap();
        }
        let before = std::fs::read(&f.service.config_path).unwrap();
        f.tick().await;
        assert_eq!(f.pulls.load(Ordering::SeqCst), 0, "{case}");
        assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
        assert!(!Journal::new(&f.service).path.exists(), "{case}");
    }
}

#[tokio::test]
async fn setup_fence_refuses_revoked_policy_model_catalog_and_runtime_drift() {
    for case in [
        "disabled",
        "policy_aba",
        "selection",
        "catalog",
        "runtime",
        "applied_cli",
    ] {
        let f = Fixture::new(false).await;
        let before = f.service.read_config().unwrap();
        let fence = Fence {
            saved: Saved::from_config(&before).unwrap(),
            backend: Backend::Ollama,
            runtime_id: f.runtime.clone(),
            models_sha256: crate::grants::definition_digest(&before["models"]).unwrap(),
        };
        let mut changed = before.clone();
        match case {
            "disabled" | "policy_aba" => changed["model_bootstrap"]["automation"] = json!(false),
            "selection" => {
                changed["models"]["ollama"]["selected"] =
                    json!([{"model_id":"owner-selection","modality":"text"}])
            }
            "applied_cli" => {
                f.service.live.lock().unwrap()["setup_existing_applied_models"] = json!(true)
            }
            "catalog" => changed["model_catalog"][0]["name"] = json!("drift"),
            "runtime" => {
                f.service.live.lock().unwrap()["runtime_id"] =
                    json!(uuid::Uuid::new_v4().to_string())
            }
            _ => unreachable!(),
        }
        f.service.save_config(changed).unwrap();
        if case == "policy_aba" {
            f.service
                .inspect_grant_configuration(|_| f.service.save_bootstrap_config_locked(before))
                .unwrap();
        }
        let bytes = std::fs::read(&f.service.config_path).unwrap();
        let rejected=f.service.begin_model_download_checked(&f.service.read_config().unwrap(),false,json!({"id":uuid::Uuid::new_v4().to_string(),"model_id":"fixture","backend":"ollama","definition_sha256":"a".repeat(64),"state":"downloading"}),Some(&fence));
        assert!(rejected.is_err(), "{case}");
        assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(&f.service.config_path).unwrap(), bytes);
        assert_ne!(
            f.service.live.lock().unwrap()["download"]["state"],
            "downloading"
        );
    }
}

#[tokio::test]
async fn setup_interrupted_attempt_is_retained_unknown_and_not_replayed() {
    let f = Fixture::new(false).await;
    let c = f.service.read_config().unwrap();
    let now = crate::protocol::now_ms();
    let record = Attempt {
        schema_version: 1,
        node_id: "node".into(),
        id: uuid::Uuid::new_v4().to_string(),
        runtime_id: f.runtime.clone(),
        saved: Saved::from_config(&c).unwrap(),
        models_sha256: crate::grants::definition_digest(&c["models"]).unwrap(),
        backend: Backend::Ollama,
        phase: Phase::Inspecting,
        reason: None,
        download_admitted: false,
        created_at_ms: now,
        updated_at_ms: now,
    };
    Journal::new(&f.service).write(&record).unwrap();
    interrupt(&f.service, &f.runtime);
    let retained = f.record();
    assert_eq!(retained.phase, Phase::Unknown);
    assert_eq!(retained.id, record.id);
    f.tick().await;
    assert_eq!(f.record().id, record.id);
    assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
    assert!(
        crate::model_selection::selection_view(&f.service.read_config().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn setup_corrupt_evidence_neither_reinitializes_nor_disables_core_settings() {
    let f = Fixture::new(false).await;
    let path = Journal::new(&f.service).path;
    let invalid = b"not a sqlite database; do not repair";
    std::fs::write(&path, invalid).unwrap();
    assert!(
        tick(f.service.clone(), &f.runtime, &Journal::new(&f.service))
            .await
            .is_err()
    );
    assert_eq!(f.status().state, State::EvidenceUnavailable);
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    let mut c = f.service.read_config().unwrap();
    c["name"] = json!("Owner core control still works");
    f.service.save_config(c).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
    assert!(f.dir.path().is_dir());
}

#[tokio::test]
async fn setup_future_schema_and_oversized_evidence_are_unchanged_on_inspection() {
    for case in ["future", "oversized"] {
        let f = Fixture::new(false).await;
        let path = Journal::new(&f.service).path;
        if case == "future" {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("PRAGMA user_version=99;").unwrap();
            drop(db);
        } else {
            std::fs::write(&path, vec![0u8; 256 * 1024 + 1]).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert_eq!(f.status().state, State::EvidenceUnavailable);
        assert!(
            tick(f.service.clone(), &f.runtime, &Journal::new(&f.service))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(f.pulls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn setup_journal_rejects_terminal_replay_and_supervisor_future_is_bounded() {
    let f = Fixture::new(true).await;
    let future = run(f.service.clone(), f.runtime.clone());
    assert!(
        std::mem::size_of_val(&future) <= 8192,
        "keep the Windows runtime supervisor frame bounded"
    );
    drop(future);
    f.tick().await;
    let journal = Journal::new(&f.service);
    let before = std::fs::read(&journal.path).unwrap();
    let mut replay = f.record();
    replay.phase = Phase::Inspecting;
    assert!(journal.write(&replay).is_err());
    replay.id = uuid::Uuid::new_v4().to_string();
    assert!(journal.write(&replay).is_err());
    assert_eq!(std::fs::read(&journal.path).unwrap(), before);
}
