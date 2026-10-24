use super::*;

#[test]
fn bootstrap_legacy_policy_digest_and_disabled_omission_are_unchanged() {
    let saved = Saved::new(Policy::default(), 0).unwrap();
    assert_eq!(
        saved.digest,
        crate::grants::definition_digest(
            &json!({"policy":{"schema_version":1,"mode":"preserve","catalog":null},"revision":0})
        )
        .unwrap()
    );
    assert!(
        serde_json::to_value(saved.policy)
            .unwrap()
            .get("automation")
            .is_none()
    );
}

#[test]
fn bootstrap_automation_requires_exact_owner_command_and_cannot_be_stale_restored() {
    let f = Fixture::new();
    let mut request = f.configure(
        Some(Mode::Auto),
        Target::Catalog {
            model_id: "fixture".into(),
        },
    );
    if let Command::Configure { automation, .. } = &mut request.command {
        *automation = Some(true);
    }
    let report = f
        .service
        .model_bootstrap_policy(request.clone(), false)
        .unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["automatic_setup_available"], true);
    assert_eq!(report["setup"]["state"], "waiting");
    let stale = f.service.read_config().unwrap();
    let mut disable = f.configure(None, Target::Keep);
    if let Command::Configure { automation, .. } = &mut disable.command {
        *automation = Some(false);
    }
    f.service.model_bootstrap_policy(disable, false).unwrap();
    let bytes = f.bytes();
    assert!(f.service.save_config(stale).is_err());
    assert_eq!(f.bytes(), bytes);
    let mut unrelated = f.service.read_config().unwrap();
    unrelated["future"]["keep"] = json!("still owner controlled");
    f.service.save_config(unrelated).unwrap();
    assert_eq!(f.show()["saved"]["policy"].get("automation"), None);
}
struct Fixture {
    _dir: tempfile::TempDir,
    service: std::sync::Arc<crate::NodeService>,
    runtime: String,
    _engine: std::sync::Arc<crate::NodeEngine>,
    guard: Option<crate::request_preview::PreviewGuard>,
}

#[test]
fn setup_stop_command_is_exact_current_consented_and_reports_no_physical_stop() {
    let f = Fixture::new();
    let mut configure = f.configure(
        Some(Mode::Prompt),
        Target::Catalog {
            model_id: "fixture".into(),
        },
    );
    if let Command::Configure { automation, .. } = &mut configure.command {
        *automation = Some(true);
    }
    f.service.model_bootstrap_policy(configure, false).unwrap();
    let previous: Saved = serde_json::from_value(f.show()["saved"].clone()).unwrap();
    let approved = f
        .service
        .model_bootstrap_policy(
            f.request(Command::Approve {
                previous: previous.clone(),
            }),
            false,
        )
        .unwrap();
    let id = approved["setup"]["attempt"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = f.request(Command::Stop {
        previous: previous.clone(),
        attempt_id: id.clone(),
    });
    let before = f.bytes();
    assert!(
        f.service
            .model_bootstrap_policy(request.clone(), true)
            .is_err()
    );
    assert_eq!(f.show()["setup"]["attempt"]["phase"], "approved");
    for field in ["node", "runtime", "epoch", "attempt"] {
        let mut wrong = request.clone();
        match field {
            "node" => wrong.expected_node_id = "different-node".into(),
            "runtime" => wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string(),
            "epoch" => {
                wrong.command = Command::Stop {
                    previous: Saved::new(previous.policy.clone(), previous.revision + 1).unwrap(),
                    attempt_id: id.clone(),
                }
            }
            "attempt" => {
                wrong.command = Command::Stop {
                    previous: previous.clone(),
                    attempt_id: uuid::Uuid::new_v4().to_string(),
                }
            }
            _ => unreachable!(),
        }
        assert!(
            f.service.model_bootstrap_policy(wrong, false).is_err(),
            "{field}"
        );
        assert_eq!(f.show()["setup"]["attempt"]["phase"], "approved");
    }
    assert_eq!(f.bytes(), before);
    let mut config = f.service.read_config().unwrap();
    config["management"]["allow_remote"] = json!(true);
    f.service.save_config(config).unwrap();
    let before = f.bytes();
    let report = f
        .service
        .model_bootstrap_policy(request.clone(), true)
        .unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["saved"], json!(previous));
    assert_eq!(report["setup"]["attempt"]["reason"], "owner_stopped");
    assert_eq!(
        report["setup_stop"]["transfer_cancellation"],
        "not_requested"
    );
    assert_eq!(report["setup_stop"]["physical_stop_verified"], false);
    assert_eq!(f.bytes(), before);
    assert!(
        render(&report)
            .unwrap()
            .contains("shared-provider shutdown")
    );
    assert!(
        f.service
            .model_bootstrap_policy(request.clone(), true)
            .is_err()
    );
    for (pointer, value) in [
        ("/setup_stop/physical_stop_verified", json!(true)),
        ("/setup_stop/configuration_rolled_back", json!(true)),
        (
            "/setup_stop/attempt_id",
            json!(uuid::Uuid::new_v4().to_string()),
        ),
        (
            "/setup_stop/transfer_cancellation",
            json!("provider_stopped"),
        ),
        ("/setup/attempt/phase", json!("configured")),
        ("/setup/attempt/reason", json!("state_changed")),
        ("/changed", json!(false)),
    ] {
        let mut bad = report.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(!confirmed(&bad, &request), "{pointer}");
        assert!(render(&bad).is_err(), "{pointer}");
    }
    let mut missing = report.clone();
    missing.as_object_mut().unwrap().remove("setup_stop");
    assert!(!confirmed(&missing, &request));
    let show_request = f.request(Command::Show);
    let mut unsolicited = f.show();
    unsolicited["setup_stop"] = report["setup_stop"].clone();
    assert!(!confirmed(&unsolicited, &show_request));
    for id in ["", "not-a-uuid", "00000000-0000-0000-0000-000000000000"] {
        assert!(
            Command::Stop {
                previous: previous.clone(),
                attempt_id: id.into()
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn prompt_approval_requires_current_remote_consent_and_retains_exact_unchanged_policy() {
    let f = Fixture::new();
    let mut configure = f.configure(
        Some(Mode::Prompt),
        Target::Catalog {
            model_id: "fixture".into(),
        },
    );
    if let Command::Configure { automation, .. } = &mut configure.command {
        *automation = Some(true);
    }
    f.service.model_bootstrap_policy(configure, false).unwrap();
    let previous: Saved = serde_json::from_value(f.show()["saved"].clone()).unwrap();
    let request = f.request(Command::Approve {
        previous: previous.clone(),
    });
    assert!(
        f.service
            .model_bootstrap_policy(request.clone(), true)
            .is_err()
    );
    assert!(
        !f.service
            .config_path
            .with_extension("bootstrap.sqlite3")
            .exists()
    );
    let mut config = f.service.read_config().unwrap();
    config["management"]["allow_remote"] = json!(true);
    f.service.save_config(config).unwrap();
    let before = f.bytes();
    let report = f
        .service
        .model_bootstrap_policy(request.clone(), true)
        .unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["changed"], true);
    assert_eq!(report["saved"], json!(previous));
    assert_eq!(report["setup"]["attempt"]["phase"], "approved");
    assert_eq!(report["setup"]["attempt"]["owner_approved"], true);
    assert_eq!(f.bytes(), before);
    assert!(
        f.service
            .model_bootstrap_policy(request.clone(), false)
            .is_err()
    );
    let mut legacy = report.clone();
    legacy["note"] = json!(LEGACY_AUTOMATION_NOTE);
    assert!(!confirmed(&legacy, &request));
    legacy["command"] = json!(Command::Show);
    legacy["changed"] = json!(false);
    assert!(confirmed(&legacy, &f.request(Command::Show)));
    let mut forged = report;
    forged["setup"]["attempt"]["owner_approved"] = json!(false);
    assert!(!confirmed(&forged, &request));
}

#[test]
fn prompt_approval_refuses_stale_scope_without_creating_or_changing_evidence() {
    for case in ["policy", "runtime", "catalog", "inactive"] {
        let f = Fixture::new();
        let mut configure = f.configure(
            Some(Mode::Prompt),
            Target::Catalog {
                model_id: "fixture".into(),
            },
        );
        if let Command::Configure { automation, .. } = &mut configure.command {
            *automation = Some(true);
        }
        f.service.model_bootstrap_policy(configure, false).unwrap();
        let mut request = f.request(Command::Approve {
            previous: serde_json::from_value(f.show()["saved"].clone()).unwrap(),
        });
        match case {
            "runtime" => request.expected_runtime_id = uuid::Uuid::new_v4().to_string(),
            "policy" | "inactive" => {
                let mut disable = f.configure(None, Target::Keep);
                if let Command::Configure { automation, .. } = &mut disable.command {
                    *automation = Some(false);
                }
                f.service.model_bootstrap_policy(disable, false).unwrap();
                if case == "inactive" {
                    request.command = Command::Approve {
                        previous: serde_json::from_value(f.show()["saved"].clone()).unwrap(),
                    };
                }
            }
            "catalog" => {
                let mut config = f.service.read_config().unwrap();
                config["model_catalog"][0]["bytes"] = json!(64);
                f.service.save_config(config).unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.bytes();
        assert!(
            f.service.model_bootstrap_policy(request, false).is_err(),
            "{case}"
        );
        assert_eq!(f.bytes(), before);
        assert!(
            !f.service
                .config_path
                .with_extension("bootstrap.sqlite3")
                .exists()
        );
    }
}

#[test]
fn bootstrap_restart_consent_is_separate_and_stale_generic_restore_cannot_reenable_it() {
    let f = Fixture::new();
    let mut request = f.configure(
        Some(Mode::Auto),
        Target::Catalog {
            model_id: "fixture".into(),
        },
    );
    if let Command::Configure { automation, .. } = &mut request.command {
        *automation = Some(true);
    }
    f.service.model_bootstrap_policy(request, false).unwrap();
    assert!(
        f.show()["saved"]["policy"]
            .get("restart_after_setup")
            .is_none()
    );
    let mut request = f.configure(None, Target::Keep);
    if let Command::Configure {
        restart_after_setup,
        ..
    } = &mut request.command
    {
        *restart_after_setup = Some(true);
    }
    let report = f
        .service
        .model_bootstrap_policy(request.clone(), false)
        .unwrap();
    assert!(confirmed(&report, &request));
    let stale = f.service.read_config().unwrap();
    assert_eq!(stale["model_bootstrap"]["restart_after_setup"], true);
    let mut request = f.configure(None, Target::Keep);
    if let Command::Configure {
        restart_after_setup,
        ..
    } = &mut request.command
    {
        *restart_after_setup = Some(false);
    }
    f.service.model_bootstrap_policy(request, false).unwrap();
    let bytes = f.bytes();
    assert!(f.service.save_config(stale).is_err());
    assert_eq!(f.bytes(), bytes);
}
fn entry() -> Value {
    json!({"id":"fixture","modality":"text","url":"https://example.invalid/model.gguf?token=PRIVATE_SOURCE_TOKEN",
        "sha256":"a".repeat(64),"bytes":1234,"license":"Apache-2.0","source_page":"https://example.invalid/source?token=PRIVATE_PAGE_TOKEN"})
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":"node","psk":"PRIVATE_NODE_SECRET","endpoints":[],
            "future":{"keep":true},"management":{"allow_remote":false},"model_catalog":[entry()]}),
        )
        .unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let engine =
            std::sync::Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            _dir: dir,
            service,
            runtime,
            _engine: engine,
            guard: Some(guard),
        }
    }
    fn request(&self, command: Command) -> Request {
        Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command,
        }
    }
    fn show(&self) -> Value {
        self.service
            .model_bootstrap_policy(self.request(Command::Show), false)
            .unwrap()
    }
    fn configure(&self, mode: Option<Mode>, target: Target) -> Request {
        self.request(Command::Configure {
            previous: serde_json::from_value(self.show()["saved"].clone()).unwrap(),
            mode,
            target,
            automation: None,
            restart_after_setup: None,
        })
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.service.config_path).unwrap()
    }
}

#[test]
fn bootstrap_legacy_inspection_never_opts_in_creates_settings_or_claims_readiness() {
    let f = Fixture::new();
    let before = f.bytes();
    let report = f.show();
    assert!(confirmed(&report, &f.request(Command::Show)));
    assert_eq!(report["saved"]["policy"]["mode"], "preserve");
    assert_eq!(report["saved"]["revision"], 0);
    assert_eq!(report["state"], "preserved");
    assert_eq!(report["binding"], "project_default_unavailable");
    assert_eq!(before, f.bytes());
    assert!(!f.service.config_path.with_extension("json.bak").exists());
    assert!(render(&report).unwrap().contains("not available"));
}

#[test]
fn bootstrap_atomic_catalog_pin_redacts_sources_and_noop_never_rewrites() {
    let f = Fixture::new();
    let request = f.configure(
        Some(Mode::Auto),
        Target::Catalog {
            model_id: "fixture".into(),
        },
    );
    let report = f
        .service
        .model_bootstrap_policy(request.clone(), false)
        .unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["changed"], true);
    assert_eq!(report["saved"]["revision"], 1);
    assert_eq!(report["state"], "automation_unavailable");
    assert_eq!(report["binding"], "matched");
    let text = serde_json::to_string(&report).unwrap();
    for secret in [
        "PRIVATE_SOURCE_TOKEN",
        "PRIVATE_PAGE_TOKEN",
        "PRIVATE_NODE_SECRET",
        "example.invalid",
    ] {
        assert!(!text.contains(secret));
    }
    let config = f.service.read_config().unwrap();
    assert_eq!(config["psk"], "PRIVATE_NODE_SECRET");
    assert_eq!(config["future"]["keep"], true);
    assert_eq!(config["management"]["allow_remote"], false);
    assert!(!config.get("models").is_some());
    let before = f.bytes();
    assert!(f.service.model_bootstrap_policy(request, false).is_err());
    let noop = f
        .service
        .model_bootstrap_policy(
            f.configure(
                Some(Mode::Auto),
                Target::Catalog {
                    model_id: "fixture".into(),
                },
            ),
            false,
        )
        .unwrap();
    assert_eq!(noop["changed"], false);
    assert_eq!(noop["saved"]["revision"], 1);
    assert_eq!(before, f.bytes());
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
}

#[test]
fn bootstrap_central_revision_preserves_omitted_optout_and_fences_generic_aba() {
    let f = Fixture::new();
    f.service
        .model_bootstrap_policy(f.configure(Some(Mode::Off), Target::Keep), false)
        .unwrap();
    let stale = f.configure(Some(Mode::Auto), Target::Keep);
    let mut config = f.service.read_config().unwrap();
    config.as_object_mut().unwrap().remove("model_bootstrap");
    config["model_bootstrap_revision"] = json!(999);
    config["future"]["added"] = json!(true);
    f.service.save_config(config).unwrap();
    assert_eq!(f.show()["saved"]["policy"]["mode"], "off");
    assert_eq!(f.show()["saved"]["revision"], 1);
    for mode in [Mode::Prompt, Mode::Off] {
        let mut config = f.service.read_config().unwrap();
        config["model_bootstrap"]["mode"] = json!(mode);
        config["model_bootstrap_revision"] = json!(0);
        f.service.save_config(config).unwrap();
    }
    assert_eq!(f.show()["saved"]["revision"], 3);
    let before = f.bytes();
    assert!(f.service.model_bootstrap_policy(stale, false).is_err());
    assert_eq!(f.bytes(), before);
    assert_eq!(f.service.read_config().unwrap()["future"]["added"], true);
}

#[test]
fn bootstrap_catalog_drift_is_visible_and_never_implicitly_rebound() {
    let f = Fixture::new();
    f.service
        .model_bootstrap_policy(
            f.configure(
                Some(Mode::Auto),
                Target::Catalog {
                    model_id: "fixture".into(),
                },
            ),
            false,
        )
        .unwrap();
    let pin = f.show()["saved"]["policy"]["catalog"].clone();
    let mut config = f.service.read_config().unwrap();
    config["model_catalog"][0]["sha256"] = json!("b".repeat(64));
    f.service.save_config(config).unwrap();
    let before = f.bytes();
    let stale = f.show();
    assert_eq!(stale["state"], "catalog_changed");
    assert_eq!(stale["saved"]["policy"]["catalog"], pin);
    assert_eq!(stale["saved"]["revision"], 1);
    assert_eq!(before, f.bytes());
    f.service
        .model_bootstrap_policy(
            f.configure(
                None,
                Target::Catalog {
                    model_id: "fixture".into(),
                },
            ),
            false,
        )
        .unwrap();
    assert_eq!(f.show()["binding"], "matched");
    assert_eq!(f.show()["saved"]["revision"], 2);
}

#[test]
fn bootstrap_rejects_invalid_or_ambiguous_catalog_without_partial_policy_save() {
    for case in 0..8 {
        let f = Fixture::new();
        let mut config = f.service.read_config().unwrap();
        match case {
            0 => config["model_catalog"][0]["url"] = json!("http://example.invalid/model.gguf"),
            1 => {
                config["model_catalog"][0]["url"] =
                    json!("https://user:secret@example.invalid/model.gguf")
            }
            2 => config["model_catalog"][0]["sha256"] = json!("missing"),
            3 => config["model_catalog"][0]["bytes"] = json!(0),
            4 => config["model_catalog"][0]["license"] = json!("bad\nlicense"),
            5 => {
                config["model_catalog"][0]["source_page"] = json!("http://example.invalid/license")
            }
            6 => config["model_catalog"][0]["modality"] = json!("vlm"),
            _ => config["model_catalog"] = json!([entry(), entry()]),
        }
        f.service.save_config(config).unwrap();
        let before = f.bytes();
        assert!(
            f.service
                .model_bootstrap_policy(
                    f.configure(
                        Some(Mode::Auto),
                        Target::Catalog {
                            model_id: "fixture".into()
                        }
                    ),
                    false
                )
                .is_err(),
            "case {case}"
        );
        assert_eq!(before, f.bytes());
        assert_eq!(f.show()["saved"]["revision"], 0);
    }
}

#[tokio::test]
async fn bootstrap_remote_write_rechecks_consent_runtime_and_reserved_status() {
    let mut f = Fixture::new();
    let request = f.configure(Some(Mode::Prompt), Target::Keep);
    let before = f.bytes();
    assert!(
        f.service
            .remote("bootstrap_policy", json!(request))
            .await
            .is_err()
    );
    assert!(
        f.service
            .remote("bootstrap_policy_status", json!(request))
            .await
            .is_err()
    );
    assert!(
        f.service
            .remote("bootstrap_policy_status", json!(f.request(Command::Show)))
            .await
            .is_ok()
    );
    assert_eq!(before, f.bytes());
    let mut wrong = request.clone();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.model_bootstrap_policy(wrong, false).is_err());
    let mut config = f.service.read_config().unwrap();
    config["management"]["allow_remote"] = json!(true);
    f.service.save_config(config).unwrap();
    let report = f
        .service
        .remote("bootstrap_policy", json!(request))
        .await
        .unwrap();
    assert!(confirmed(&report, &request));
    f.guard.take();
    assert!(
        f.service
            .model_bootstrap_policy(f.request(Command::Show), false)
            .is_err()
    );
}

#[test]
fn bootstrap_report_refuses_forged_authority_readiness_versions_and_unknown_fields() {
    let f = Fixture::new();
    let request = f.request(Command::Show);
    let report = f.show();
    for field in [
        "automatic_setup_available",
        "project_default_approved",
        "license_reviewed",
        "runtime_compatibility_verified",
        "download_started",
        "selection_changed",
        "model_inference_requested",
        "execution_authorized",
        "restart_requested",
        "automatically_retried",
        "changed",
    ] {
        let mut bad = report.clone();
        bad[field] = json!(true);
        assert!(!confirmed(&bad, &request), "{field}");
    }
    for (pointer, value) in [
        ("/saved/revision", json!(1)),
        ("/saved/digest", json!("b".repeat(64))),
        ("/state", json!("automation_unavailable")),
        ("/binding", json!("matched")),
        ("/interpreter_count", json!(1)),
    ] {
        let mut bad = report.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(!confirmed(&bad, &request), "{pointer}");
    }
    let mut bad = report;
    bad["grant"] = json!("forged");
    assert!(!confirmed(&bad, &request));
}

#[test]
fn bootstrap_mode_changes_preserve_existing_models_and_never_select_a_default() {
    let f = Fixture::new();
    let mut config = f.service.read_config().unwrap();
    config["models"] = json!({"provider":"ollama","ollama":{"selected":[{"model_id":"owner","modality":"text","selection":{"roles":["request_interpreter"]}}]}});
    f.service.save_config(config.clone()).unwrap();
    for (mode, expected) in [
        (Mode::Auto, "interpreter_selected"),
        (Mode::Managed, "managed"),
        (Mode::Off, "disabled"),
        (Mode::Preserve, "preserved"),
    ] {
        let report = f
            .service
            .model_bootstrap_policy(f.configure(Some(mode), Target::ProjectDefault), false)
            .unwrap();
        assert_eq!(report["state"], expected);
        assert_eq!(report["saved"]["policy"]["catalog"], Value::Null);
        assert_eq!(f.service.read_config().unwrap()["models"], config["models"]);
    }
    let mut config = f.service.read_config().unwrap();
    config["models"]["ollama"]["selected"][0]["selection"]["capabilities"] = json!([]);
    f.service.save_config(config).unwrap();
    let report = f
        .service
        .model_bootstrap_policy(f.configure(Some(Mode::Auto), Target::Keep), false)
        .unwrap();
    assert_eq!(report["state"], "existing_selections");
    assert_eq!(report["interpreter_count"], 0);
}

#[test]
fn bootstrap_revision_exhaustion_and_invalid_versions_never_repair_or_overwrite() {
    let f = Fixture::new();
    let mut config = f.service.read_config().unwrap();
    config["model_bootstrap_revision"] = json!(u64::MAX);
    crate::setup::write_json(&f.service.config_path, &config).unwrap();
    let request = f.configure(Some(Mode::Auto), Target::Keep);
    let before = f.bytes();
    assert!(f.service.model_bootstrap_policy(request, false).is_err());
    assert_eq!(before, f.bytes());
    config["model_bootstrap_revision"] = json!("damaged");
    crate::setup::write_json(&f.service.config_path, &config).unwrap();
    let before = f.bytes();
    assert!(
        f.service
            .model_bootstrap_policy(f.request(Command::Show), false)
            .is_err()
    );
    assert_eq!(before, f.bytes());
}

#[test]
fn bootstrap_explicit_auto_without_a_target_never_chooses_the_first_catalog_entry() {
    let f = Fixture::new();
    let report = f
        .service
        .model_bootstrap_policy(f.configure(Some(Mode::Auto), Target::ProjectDefault), false)
        .unwrap();
    assert_eq!(report["state"], "no_approved_default");
    assert_eq!(report["saved"]["policy"]["catalog"], Value::Null);
    assert_eq!(report["download_started"], false);
    assert_eq!(report["selection_count"], 0);
    assert!(f.service.read_config().unwrap().get("models").is_none());
}

#[test]
fn bootstrap_concurrent_commands_cannot_both_save_from_the_same_policy_version() {
    let f = Fixture::new();
    let first = f.configure(Some(Mode::Auto), Target::Keep);
    let second = f.configure(Some(Mode::Off), Target::Keep);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| f.service.model_bootstrap_policy(first, false));
        let b = scope.spawn(|| f.service.model_bootstrap_policy(second, false));
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(f.show()["saved"]["revision"], 1);
    assert_eq!(f.service.read_config().unwrap()["future"]["keep"], true);
}

#[test]
fn bootstrap_opaque_future_or_damaged_policy_does_not_disable_unrelated_owner_saves() {
    for case in 0..3 {
        let f = Fixture::new();
        let mut config = f.service.read_config().unwrap();
        match case {
            0 => {
                config["model_bootstrap"] =
                    json!({"schema_version":99,"mode":"future","opaque":{"keep":true}});
                config["model_bootstrap_revision"] = json!(42);
            }
            1 => config["model_bootstrap_revision"] = json!("damaged"),
            _ => config["model_bootstrap"] = Value::Null,
        }
        crate::setup::write_json(&f.service.config_path, &config).unwrap();
        let mut next = config.clone();
        next.as_object_mut().unwrap().remove("model_bootstrap");
        next["model_bootstrap_revision"] = json!(0);
        next["future"]["owner_edit"] = json!(true);
        f.service.save_config(next).unwrap();
        let saved = f.service.read_config().unwrap();
        assert_eq!(saved.get("model_bootstrap"), config.get("model_bootstrap"));
        assert_eq!(
            saved.get("model_bootstrap_revision"),
            config.get("model_bootstrap_revision")
        );
        assert_eq!(saved["future"]["owner_edit"], true);
        assert!(
            f.service
                .model_bootstrap_policy(f.request(Command::Show), false)
                .is_err()
        );
        let before = f.bytes();
        let mut repair = saved;
        repair["model_bootstrap"] = json!(Policy::default());
        assert!(f.service.save_config(repair).is_err());
        assert_eq!(before, f.bytes());
    }
    let f = Fixture::new();
    let mut unsupported = f.service.read_config().unwrap();
    unsupported["model_bootstrap"] = json!({"schema_version":99});
    let before = f.bytes();
    assert!(f.service.save_config(unsupported).is_err());
    assert_eq!(before, f.bytes());
}
