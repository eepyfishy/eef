use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;

use crate::model_metadata::{ModelAvailability, ModelLifecycle};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, watch};
use tokio::time::sleep;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelSlot {
    pub model_id: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub model_path: PathBuf,
    #[serde(default)]
    pub mmproj_path: Option<PathBuf>,
    #[serde(default)]
    pub gpu_layers: i32,
    #[serde(default = "default_context")]
    pub context: u32,
    /// Requested generation/prompt worker count, not a CPU-percent or RAM quota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_threads: Option<u16>,
    /// Windows owned-job committed-memory ceiling; not RAM/working set/VRAM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_memory_limit_mib: Option<u64>,
    #[serde(default)]
    pub gpu_vram_mb: u64,
    #[serde(
        default,
        rename = "selection",
        skip_serializing_if = "crate::model_selection::ModelHints::is_empty"
    )]
    pub hints: crate::model_selection::ModelHints,
}

const fn default_port() -> u16 {
    8082
}
const fn default_context() -> u32 {
    4096
}

impl ModelSlot {
    pub fn selection_metadata(&self) -> Result<crate::model_metadata::ModelMetadata> {
        validate_cpu_threads(self.cpu_threads)?;
        crate::model_memory::validate(self.job_memory_limit_mib)?;
        self.hints
            .metadata(if self.is_vlm() { "vlm" } else { "text" })
    }
    pub fn is_vlm(&self) -> bool {
        self.mmproj_path
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty())
    }

    pub fn advertise(&self) -> Result<Value> {
        let modality = if self.is_vlm() { "vlm" } else { "text" };
        let mut value = json!({
            "model_id": self.model_id,
            "backend": "llamacpp",
            "hardware": if self.gpu_layers != 0 { "gpu" } else { "cpu" },
            "vram_mb": self.gpu_vram_mb,
            "port": self.port,
            "modality": modality,
            "model_metadata": self.selection_metadata()?,
        });
        if let Some(threads) = self.cpu_threads {
            value["cpu_threads"] = json!(threads);
        }
        if let Some(limit) = self.job_memory_limit_mib {
            value["job_memory_limit_mib"] = json!(limit);
        }
        Ok(value)
    }
}

pub(crate) fn validate_cpu_threads(threads: Option<u16>) -> Result<()> {
    if threads.is_some_and(|n| !(1..=256).contains(&n)) {
        bail!("CPU threads must be 1-256, or omitted for backend defaults")
    }
    Ok(())
}

fn configure_cpu_threads(command: &mut Command, threads: Option<u16>) -> Result<()> {
    validate_cpu_threads(threads)?;
    if let Some(threads) = threads {
        // Set both: inherited environment must not widen prompt worker counts.
        command.arg("--threads").arg(threads.to_string());
        command.arg("--threads-batch").arg(threads.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod advertisement_tests {
    use super::*;
    #[tokio::test]
    async fn concurrent_demand_waiters_share_one_load_and_atomic_active_pin_handoff() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"owner","models":{"provider":"llamacpp"},"model_lifecycle":{"startup":"on_demand"}})).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let healthy = Arc::new(tokio::sync::Notify::new());
        let release = healthy.clone();
        let http = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            socket.read(&mut request).await.unwrap();
            healthy.notified().await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });
        let artifact = directory.path().join("synthetic.gguf");
        std::fs::write(&artifact, b"test-only").unwrap();
        let slot = serde_json::from_value(
            json!({"model_id":"selected","port":port,"model_path":artifact}),
        )
        .unwrap();
        let server = ModelServer::new(std::env::current_exe().unwrap(), vec![slot]);
        let service = crate::NodeService::new(path, "owner".into());
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        let starts = std::sync::atomic::AtomicUsize::new(0);
        let spawn = |_: &ModelSlot, _: &str| {
            starts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            server.test_spawn_resident("selected")
        };
        let (one, two, _) = tokio::join!(
            server.acquire_demand_pin("selected", Duration::from_secs(3), spawn, || Ok(())),
            server.acquire_demand_pin("selected", Duration::from_secs(3), spawn, || Ok(())),
            async {
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if server.snapshot().unwrap()[0]["state"] == "loading" {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert!(server.can_infer(Some("selected")));
                release.notify_one();
            }
        );
        let one = one.unwrap();
        let two = two.unwrap();
        assert_eq!(starts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 2);
        assert_eq!(server.snapshot().unwrap()[0]["state"], "active");
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        assert!(
            server
                .reserve_control(
                    "selected",
                    generation,
                    &uuid::Uuid::new_v4().to_string(),
                    false
                )
                .is_err()
        );
        drop(one);
        drop(two);
        assert_eq!(server.snapshot().unwrap()[0]["state"], "warm_idle");
        http.await.unwrap();
        drop(guard);
    }
    #[tokio::test]
    async fn final_load_authority_refusal_cleans_owned_process_before_any_pin() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let http = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });
        let slot = serde_json::from_value(json!({"model_id":"selected","port":port})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        let id = uuid::Uuid::new_v4().to_string();
        server.reserve_control("selected", 0, &id, true).unwrap();
        let confirmed = std::sync::atomic::AtomicBool::new(false);
        assert!(
            server
                .load_reserved_checked(
                    "selected",
                    &id,
                    Duration::from_secs(3),
                    |_| server.test_spawn_resident("selected"),
                    || {
                        confirmed.store(true, std::sync::atomic::Ordering::SeqCst);
                        Err::<(), _>(anyhow::anyhow!(
                            "current authority changed before readiness"
                        ))
                    }
                )
                .await
                .is_err()
        );
        assert!(
            confirmed.load(std::sync::atomic::Ordering::SeqCst),
            "must reach final authority, not merely time out"
        );
        let row = server.snapshot().unwrap();
        assert_eq!(row[0]["state"], "error");
        assert_eq!(row[0]["active_requests"], 0);
        assert!(row[0]["owned_pid"].is_null());
        http.await.unwrap();
        server.close();
    }
    #[tokio::test]
    async fn owned_process_exit_before_health_handoff_never_fabricates_readiness() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let http = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });
        let slot = serde_json::from_value(json!({"model_id":"selected","port":port})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        let id = uuid::Uuid::new_v4().to_string();
        server.reserve_control("selected", 0, &id, true).unwrap();
        let final_check = std::sync::atomic::AtomicBool::new(false);
        let result = server
            .load_reserved_checked(
                "selected",
                &id,
                Duration::from_secs(3),
                |_| server.test_spawn_resident("selected"),
                || {
                    final_check.store(true, std::sync::atomic::Ordering::SeqCst);
                    let mut processes = server.processes.lock().unwrap();
                    let owned = processes.get_mut("selected").unwrap();
                    owned.child.start_kill()?;
                    let deadline = std::time::Instant::now() + Duration::from_secs(1);
                    loop {
                        if owned.child.try_wait()?.is_some() {
                            break;
                        }
                        if std::time::Instant::now() >= deadline {
                            bail!("test child termination unconfirmed")
                        };
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                },
            )
            .await;
        assert!(final_check.load(std::sync::atomic::Ordering::SeqCst));
        assert!(result.is_err());
        let row = server.snapshot().unwrap();
        assert_eq!(row[0]["state"], "error");
        assert_eq!(row[0]["active_requests"], 0);
        assert!(row[0]["owned_pid"].is_null());
        http.await.unwrap();
        server.close();
    }
    #[test]
    fn resident_child_helper() {
        if std::env::args().any(|s| s == "eef-owned-model-lifecycle-test") {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    pub(super) fn resident_server() -> Arc<ModelServer> {
        let slot = serde_json::from_value(json!({"model_id":"owned-model"})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        server.initialize_residency().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "model_server::advertisement_tests::resident_child_helper",
                "--skip",
                "eef-owned-model-lifecycle-test",
                "--test-threads=1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (child, tree) = crate::process_lifetime::spawn(&mut command).unwrap();
        server
            .processes
            .lock()
            .unwrap()
            .insert("owned-model".into(), OwnedProcess { child, _tree: tree });
        server.set_all(ModelLifecycle::Ready);
        server
    }
    #[tokio::test]
    async fn snapshots_distinguish_missing_job_from_observed_owned_resources() {
        let server = resident_server();
        let before = server.snapshot().unwrap();
        let first = before[0].clone();
        #[cfg(windows)]
        {
            assert_eq!(first["job_accounting"]["state"], "available");
            assert!(
                first["job_accounting"]["peak_job_commit_bytes"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(
                first["job_accounting"]["job_commit_limit_bytes"],
                Value::Null
            );
        }
        #[cfg(not(windows))]
        assert_eq!(first["job_accounting"]["state"], "unsupported");
        assert_eq!(
            server.snapshot().unwrap()[0]["generation"],
            first["generation"]
        );
        assert_eq!(
            server.snapshot().unwrap()[0]["owned_pid"],
            first["owned_pid"]
        );
        server.stop().await.unwrap();
        assert_eq!(
            server.snapshot().unwrap()[0]["job_accounting"],
            json!({"state":"no_owned_job"})
        );
    }
    #[tokio::test]
    async fn inference_pins_block_unload_and_close_fences_old_pins_and_late_loads() {
        let server = resident_server();
        let one = server.pin(Some("owned-model")).unwrap();
        let two = server.pin(Some("owned-model")).unwrap();
        assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 2);
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "active"
        );
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(
            server
                .reserve_control("owned-model", generation, &id, false)
                .is_err()
        );
        drop(one);
        assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 1);
        drop(two);
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "warm_idle"
        );
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        assert!(server.pin(Some("owned-model")).is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "unavailable"
        );
        server.release_control("owned-model", &id);
        let old_pin = server.pin(Some("owned-model")).unwrap();
        server.close();
        drop(old_pin);
        assert!(!server.model_ready(Some("owned-model")));
        assert!(server.start(Duration::from_secs(1)).await.is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "error"
        );
    }
    #[tokio::test]
    async fn unload_owns_exact_generation_and_never_reuses_an_old_reservation() {
        let server = resident_server();
        let id = uuid::Uuid::new_v4().to_string();
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        assert!(
            server
                .reserve_control("owned-model", generation + 1, &id, false)
                .is_err()
        );
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        server
            .unload_reserved("owned-model", &id, || Ok(()))
            .await
            .unwrap();
        assert!(!server.model_ready(Some("owned-model")));
        let unloaded = server.snapshot().unwrap();
        assert_eq!(unloaded[0]["state"], "unloaded");
        assert!(unloaded[0]["owned_pid"].is_null());
        assert!(
            server
                .unload_reserved("owned-model", &id, || Ok(()))
                .await
                .is_err()
        );
        assert!(
            server
                .reserve_control(
                    "owned-model",
                    generation,
                    &uuid::Uuid::new_v4().to_string(),
                    true
                )
                .is_err()
        );
    }
    #[tokio::test]
    async fn idle_reservation_rechecks_elapsed_activity_and_blocks_new_pins_atomically() {
        let server = resident_server();
        let idle = Duration::from_secs(1);
        server.test_age_idle(Duration::from_secs(2));
        let candidates = server.idle_candidates(idle).unwrap();
        assert_eq!(candidates.len(), 1);
        let (model, generation) = &candidates[0];
        let operation = uuid::Uuid::new_v4().to_string();
        let pin = server.pin(Some(model)).unwrap();
        assert!(server.idle_candidates(idle).unwrap().is_empty());
        assert!(
            server
                .reserve_idle(model, *generation, idle, &operation)
                .is_err()
        );
        drop(pin);
        assert!(
            server
                .reserve_idle(model, *generation, idle, &operation)
                .is_err(),
            "a fresh last-use event invalidates the earlier candidate without a process generation change"
        );
        server.test_age_idle(Duration::from_secs(2));
        server
            .reserve_idle(model, *generation, idle, &operation)
            .unwrap();
        assert!(server.pin(Some(model)).is_err());
        assert!(server.idle_candidates(idle).unwrap().is_empty());
        server
            .unload_reserved_with_timeout(model, &operation, Duration::from_secs(2), || Ok(()))
            .await
            .unwrap();
        assert!(server.snapshot().unwrap()[0]["owned_pid"].is_null());
        assert!(server.idle_candidates(idle).unwrap().is_empty());
        server.close();
    }
    #[tokio::test]
    async fn unload_withdraws_routing_until_observation_and_holds_its_generation_fence() {
        let server = resident_server();
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let unload = server.unload_reserved_checked(
            "owned-model",
            &id,
            Duration::from_secs(3),
            || Ok(()),
            |process, deadline| {
                let release = release.clone();
                Box::pin(async move {
                    release.notified().await;
                    process.terminate_until(deadline).await
                })
            },
        );
        tokio::pin!(unload);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut unload)
                .await
                .is_err()
        );
        let row = server.snapshot().unwrap();
        assert_eq!(row[0]["state"], "unloading");
        assert_eq!(row[0]["generation"], generation + 1);
        assert_eq!(row[0]["control_pending"], true);
        assert_eq!(row[0]["job_accounting"], json!({"state":"unavailable"}));
        assert_eq!(*server.subscribe().borrow(), ModelLifecycle::Unloading);
        assert!(!server.can_infer(Some("owned-model")));
        assert!(server.pin(Some("owned-model")).is_err());
        assert!(
            server
                .reserve_control("owned-model", generation + 1, "replacement", true)
                .is_err()
        );
        let advertised = server.models().unwrap();
        // Keep older registration readers alive with a conservative busy
        // projection; the owner lifecycle command retains the exact state.
        assert_eq!(advertised[0]["model_metadata"]["lifecycle"], "loading");
        assert_eq!(
            advertised[0]["model_metadata"]["availability"],
            "unavailable"
        );
        release.notify_one();
        unload.await.unwrap();
        let row = server.snapshot().unwrap();
        assert_eq!(row[0]["state"], "unloaded");
        assert_eq!(row[0]["generation"], generation + 1);
        assert_eq!(row[0]["control_pending"], false);
        assert_eq!(row[0]["job_accounting"], json!({"state":"no_owned_job"}));
        server.close();
    }
    #[tokio::test]
    async fn unsuccessful_or_cancelled_unload_never_claims_observed_exit_or_allows_reload() {
        for mode in ["error", "timeout", "cancel", "missing", "late"] {
            let server = resident_server();
            let generation = server.snapshot().unwrap()[0]["generation"]
                .as_u64()
                .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            server
                .reserve_control("owned-model", generation, &id, false)
                .unwrap();
            if mode == "missing" {
                server.processes.lock().unwrap().remove("owned-model");
            }
            {
                let unload = server.unload_reserved_checked(
                    "owned-model",
                    &id,
                    Duration::from_millis(40),
                    || Ok(()),
                    |process, _| {
                        Box::pin(async move {
                            let _owned = process;
                            if mode == "error" {
                                bail!("synthetic observation failure")
                            }
                            if mode == "late" {
                                // Timeout futures may poll an immediately ready
                                // result first after a non-yielding OS operation.
                                std::thread::sleep(Duration::from_millis(60));
                                return Ok(());
                            }
                            std::future::pending::<Result<()>>().await
                        })
                    },
                );
                tokio::pin!(unload);
                if mode == "cancel" {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(10), &mut unload)
                            .await
                            .is_err()
                    );
                    assert_eq!(server.snapshot().unwrap()[0]["state"], "unloading");
                } else {
                    assert!(unload.await.is_err(), "{mode}");
                }
            }
            server.release_control("owned-model", &id);
            let row = server.snapshot().unwrap();
            assert_eq!(row[0]["state"], "unload_unconfirmed", "{mode}");
            assert_eq!(row[0]["generation"], generation + 1);
            assert_eq!(row[0]["control_pending"], false);
            assert_eq!(
                server.models().unwrap()[0]["model_metadata"]["lifecycle"],
                "error"
            );
            assert!(!server.can_infer(Some("owned-model")));
            assert!(
                server
                    .reserve_control("owned-model", generation + 1, "new-load", true)
                    .is_err()
            );
            assert!(server.start(Duration::from_secs(1)).await.is_err());
            assert_eq!(server.snapshot().unwrap()[0]["state"], "unload_unconfirmed");
            for group_state in [
                ModelLifecycle::Error,
                ModelLifecycle::Unloaded,
                ModelLifecycle::Ready,
            ] {
                server.set_all(group_state);
                assert_eq!(server.snapshot().unwrap()[0]["state"], "unload_unconfirmed");
                assert!(!server.can_infer(Some("owned-model")));
                let current = server.snapshot().unwrap()[0]["generation"]
                    .as_u64()
                    .unwrap();
                assert!(
                    server
                        .reserve_control("owned-model", current, "new-load", true)
                        .is_err()
                );
            }
            server.close();
            if mode == "missing" {
                assert!(
                    server.stop().await.is_err(),
                    "missing handles cannot become exit proof"
                );
            } else {
                assert!(!server.retired.lock().unwrap().is_empty());
                server.stop().await.unwrap();
                assert!(server.retired.lock().unwrap().is_empty());
                server.stop().await.unwrap();
            }
        }
    }
    #[tokio::test]
    async fn retired_live_process_cannot_be_replaced_as_an_ordinary_load_error() {
        let server = resident_server();
        server.set_all(ModelLifecycle::Error);
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        assert!(
            server
                .reserve_control("owned-model", generation, "resident-retry", true)
                .is_err()
        );
        let process = server
            .processes
            .lock()
            .unwrap()
            .remove("owned-model")
            .unwrap();
        // Keep this synthetic process live to verify admission, rather than
        // depending on how quickly a requested OS termination happens.
        server
            .retired
            .lock()
            .unwrap()
            .push(("owned-model".into(), process));
        server.set_all(ModelLifecycle::Error);
        let row = server.snapshot().unwrap();
        assert_eq!(row[0]["job_accounting"], json!({"state":"unavailable"}));
        let generation = row[0]["generation"].as_u64().unwrap();
        assert!(
            server
                .reserve_control("owned-model", generation, "retry", true)
                .is_err()
        );
        assert!(server.start(Duration::from_secs(1)).await.is_err());
        server.close();
        server.stop().await.unwrap();
        assert!(server.retired.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn process_monitor_serializes_failure_cleanup_and_keeps_group_unroutable() {
        let server = resident_server();
        let operation = server.operation.lock().await;
        server
            .processes
            .lock()
            .unwrap()
            .get_mut("owned-model")
            .unwrap()
            .child
            .start_kill()
            .unwrap();
        let monitor = server.monitor_processes();
        tokio::pin!(monitor);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut monitor)
                .await
                .is_err()
        );
        assert!(server.retired.lock().unwrap().is_empty());
        drop(operation);
        assert!(monitor.await.is_err());
        assert_eq!(server.snapshot().unwrap()[0]["state"], "error");
        assert!(!server.can_infer(Some("owned-model")));
        assert!(server.retired.lock().unwrap().is_empty());
        server.close();
        server.stop().await.unwrap();
    }
    #[tokio::test]
    async fn group_stop_budget_includes_operation_admission_without_starting_a_new_wait() {
        let server = resident_server();
        server
            .termination_wait_seconds
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let initial = server.snapshot().unwrap();
        let operation = server.operation.lock().await;
        let result = tokio::time::timeout(Duration::from_secs(2), server.stop())
            .await
            .unwrap();
        assert!(result.is_err());
        let after = server.snapshot().unwrap();
        assert_eq!(after[0]["generation"], initial[0]["generation"]);
        assert_eq!(after[0]["owned_pid"], initial[0]["owned_pid"]);
        assert!(server.retired.lock().unwrap().is_empty());
        drop(operation);
        server.close();
        server.stop().await.unwrap();
        assert!(server.retired.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn cancelled_or_failed_group_stop_retains_ownership_for_observed_cleanup() {
        for mode in ["cancel", "timeout", "error", "late"] {
            let server = resident_server();
            let initial = server.snapshot().unwrap()[0]["owned_pid"].as_u64().unwrap();
            {
                let stop = server.stop_children_checked(Duration::from_millis(40), async {
                    if mode == "error" {
                        bail!("synthetic group observation failure")
                    }
                    if mode == "late" {
                        std::thread::sleep(Duration::from_millis(60));
                        return Ok(());
                    }
                    std::future::pending::<Result<()>>().await
                });
                tokio::pin!(stop);
                if mode == "cancel" {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(10), &mut stop)
                            .await
                            .is_err()
                    );
                    let row = server.snapshot().unwrap();
                    assert_eq!(row[0]["state"], "unloading");
                    assert_eq!(row[0]["control_pending"], true);
                    assert_eq!(row[0]["job_accounting"], json!({"state":"unavailable"}));
                    assert!(!server.can_infer(Some("owned-model")));
                } else {
                    assert!(stop.await.is_err());
                }
            }
            assert_eq!(server.snapshot().unwrap()[0]["state"], "unload_unconfirmed");
            {
                let mut retired = server.retired.lock().unwrap();
                assert_eq!(retired.len(), 1, "{mode}: retain the owned process record");
                let (model, process) = &mut retired[0];
                assert_eq!(model, "owned-model");
                // Tokio clears id() once the root has been reaped. A late
                // observation can reap that root before failing its deadline;
                // the retained record/tree must still survive that failure.
                if let Some(pid) = process.child.id() {
                    assert_eq!(u64::from(pid), initial, "{mode}: same live root");
                } else {
                    assert!(
                        process.child.try_wait().unwrap().is_some(),
                        "{mode}: a missing PID requires observed root exit"
                    );
                }
            }
            assert!(!server.can_infer(Some("owned-model")));
            server.close();
            server.stop().await.unwrap();
            assert!(server.retired.lock().unwrap().is_empty());
            assert!(
                server
                    .exit_observed
                    .load(std::sync::atomic::Ordering::SeqCst)
            );
            server.stop().await.unwrap();
        }
    }
    #[tokio::test]
    async fn runtime_or_group_withdrawal_fences_late_unload_observation() {
        for close in [true, false] {
            let server = resident_server();
            let generation = server.snapshot().unwrap()[0]["generation"]
                .as_u64()
                .unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            server
                .reserve_control("owned-model", generation, &id, false)
                .unwrap();
            let release = Arc::new(tokio::sync::Notify::new());
            let unload = server.unload_reserved_checked(
                "owned-model",
                &id,
                Duration::from_secs(3),
                || Ok(()),
                |process, deadline| {
                    let release = release.clone();
                    Box::pin(async move {
                        release.notified().await;
                        process.terminate_until(deadline).await
                    })
                },
            );
            tokio::pin!(unload);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut unload)
                    .await
                    .is_err()
            );
            if close {
                server.close();
            } else {
                server.set_all(ModelLifecycle::Error);
            }
            let closed = server.residency.lock().unwrap().slots["owned-model"].clone();
            assert_eq!(closed.state, ModelLifecycle::UnloadUnconfirmed);
            release.notify_one();
            assert!(unload.await.is_err());
            let after = server.residency.lock().unwrap().slots["owned-model"].clone();
            assert_eq!(after.state, closed.state);
            assert_eq!(after.generation, closed.generation);
            assert_eq!(after.control, closed.control);
            assert_eq!(after.active, closed.active);
            assert!(
                server
                    .reserve_control("owned-model", generation + 2, "new-load", true)
                    .is_err()
            );
            server.close();
        }
    }
    #[tokio::test]
    async fn unload_rechecks_authority_after_waiting_and_preserves_residency_on_denial() {
        let server = resident_server();
        let before = server.snapshot().unwrap();
        let generation = before[0]["generation"].as_u64().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        let operation = server.operation.lock().await;
        let checked = std::sync::atomic::AtomicBool::new(false);
        let unload = server.unload_reserved("owned-model", &id, || {
            checked.store(true, std::sync::atomic::Ordering::SeqCst);
            Err::<(), _>(anyhow::anyhow!("owner authority revoked"))
        });
        tokio::pin!(unload);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut unload)
                .await
                .is_err()
        );
        assert!(!checked.load(std::sync::atomic::Ordering::SeqCst));
        drop(operation);
        assert!(unload.await.is_err());
        assert!(checked.load(std::sync::atomic::Ordering::SeqCst));
        server.release_control("owned-model", &id);
        let mut after = server.snapshot().unwrap();
        let mut before = before;
        for rows in [&mut before, &mut after] {
            // A denied mutation preserves residency, not the counters of a
            // running process. Validate observations separately from state.
            let accounting = rows[0]
                .as_object_mut()
                .unwrap()
                .remove("job_accounting")
                .unwrap();
            let sample: crate::job_accounting::JobAccounting =
                serde_json::from_value(accounting).unwrap();
            assert!(sample.validate());
            #[cfg(windows)]
            assert!(matches!(
                sample,
                crate::job_accounting::JobAccounting::Available { .. }
            ));
        }
        assert_eq!(after, before);
        assert!(server.pin(Some("owned-model")).is_ok());
        server.close();
    }
    #[tokio::test]
    async fn withdrawn_load_reservation_cannot_spawn_a_late_child() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = directory.path().join("synthetic.gguf");
        std::fs::write(&artifact, b"not real weights").unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"selected","model_path":artifact})).unwrap();
        let server = ModelServer::new(std::env::current_exe().unwrap(), vec![slot]);
        let id = uuid::Uuid::new_v4().to_string();
        server.reserve_control("selected", 0, &id, true).unwrap();
        let result = server
            .load_reserved("selected", &id, Duration::from_secs(1), |slot| {
                // An independently observed sibling failure withdraws group ownership.
                server.set_all(ModelLifecycle::Error);
                server.spawn_slot(slot, Some(&id))
            })
            .await;
        assert!(result.is_err());
        let rows = server.snapshot().unwrap();
        assert!(rows[0]["owned_pid"].is_null());
        assert_eq!(rows[0]["state"], "error");
        assert_eq!(rows[0]["control_pending"], false);
        server.close();
    }
    #[test]
    fn explicit_threads_cover_both_worker_pools_and_legacy_settings_stay_unknown() {
        for threads in [None, Some(1), Some(4), Some(256)] {
            let mut command = Command::new("test-only-not-spawned");
            configure_cpu_threads(&mut command, threads).unwrap();
            let args: Vec<_> = command
                .as_std()
                .get_args()
                .map(|v| v.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                args,
                threads
                    .map(|n| vec![
                        "--threads".into(),
                        n.to_string(),
                        "--threads-batch".into(),
                        n.to_string()
                    ])
                    .unwrap_or_default()
            );
            let slot: ModelSlot =
                serde_json::from_value(json!({"model_id":"legacy","cpu_threads":threads})).unwrap();
            assert_eq!(slot.advertise().unwrap()["cpu_threads"], json!(threads));
            assert!(
                slot.selection_metadata()
                    .unwrap()
                    .resource_estimates
                    .ram_mb
                    .is_none()
            );
        }
        for threads in [0, 257] {
            let mut command = Command::new("test-only-not-spawned");
            assert!(configure_cpu_threads(&mut command, Some(threads)).is_err());
            assert_eq!(command.as_std().get_args().count(), 0);
            let slot: ModelSlot =
                serde_json::from_value(json!({"model_id":"invalid","cpu_threads":threads}))
                    .unwrap();
            assert!(slot.selection_metadata().is_err());
        }
    }
    #[tokio::test]
    async fn failed_group_is_visible_but_never_routable() {
        let fixture = tempfile::tempdir().unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"fixture","model_path":"PRIVATE"})).unwrap();
        let server = ModelServer::new(fixture.path().join("missing.exe"), vec![slot]);
        let mut changes = server.subscribe();
        assert!(server.require_ready().is_err());
        assert!(server.start(Duration::from_secs(1)).await.is_err());
        assert_eq!(*changes.borrow_and_update(), ModelLifecycle::Error);
        let models = server.models().unwrap();
        assert!(!models[0].to_string().contains("PRIVATE"));
        let metadata: crate::model_metadata::ModelMetadata =
            serde_json::from_value(models[0]["model_metadata"].clone()).unwrap();
        assert!(!metadata.routable());
        assert!(server.require_ready().is_err());
        assert!(server.processes.lock().unwrap().is_empty());
        server.stop().await.unwrap();
        assert_eq!(*changes.borrow(), ModelLifecycle::Unloaded);
        let invalid = serde_json::from_value(json!({
            "model_id":"invalid-hints", "selection":{"roles":["not a valid role"]}
        }))
        .unwrap();
        let invalid_server = ModelServer::new(fixture.path().join("missing.exe"), vec![invalid]);
        assert!(invalid_server.models().unwrap().is_empty());
        assert!(invalid_server.start(Duration::from_secs(1)).await.is_err());
        assert!(invalid_server.models().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelled_startup_clears_state_even_with_retained_server() {
        let server = ModelServer::new(PathBuf::new(), vec![]);
        server.lifecycle.send_replace(ModelLifecycle::Loading);
        {
            let _cleanup = StartupCleanup {
                server: &server,
                armed: true,
            };
        }
        assert_eq!(*server.subscribe().borrow(), ModelLifecycle::Unloaded);
        assert!(server.require_ready().is_err());
    }

    #[tokio::test]
    async fn missing_owned_process_withdraws_ready_group() {
        let slot = serde_json::from_value(json!({"model_id":"missing-process"})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        server.initialize_residency().unwrap();
        server.set_all(ModelLifecycle::Ready);
        let mut changes = server.subscribe();
        assert!(server.monitor_processes().await.is_err());
        assert!(changes.has_changed().unwrap());
        assert_eq!(*changes.borrow_and_update(), ModelLifecycle::Error);
        assert!(server.require_ready().is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "unavailable"
        );
        assert!(server.processes.lock().unwrap().is_empty());
        // No automatic restart/retry and no replacement of a terminal error.
        server.monitor_processes().await.unwrap();
        assert_eq!(*changes.borrow(), ModelLifecycle::Error);
    }

    #[tokio::test]
    async fn process_monitor_does_not_change_non_ready_states_or_block_stop() {
        let server = ModelServer::new(PathBuf::new(), vec![]);
        for state in [
            ModelLifecycle::Unloaded,
            ModelLifecycle::Loading,
            ModelLifecycle::Error,
        ] {
            server.lifecycle.send_replace(state);
            server.monitor_processes().await.unwrap();
            assert_eq!(*server.subscribe().borrow(), state);
        }
        server.lifecycle.send_replace(ModelLifecycle::Ready);
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(server.monitor_processes(), async {
                sleep(Duration::from_millis(20)).await;
                server.stop().await.unwrap();
            })
            .0
            .unwrap();
        })
        .await
        .unwrap();
        assert_eq!(*server.subscribe().borrow(), ModelLifecycle::Unloaded);
    }

    #[test]
    fn gguf_advertises_compatible_metadata_without_private_paths_or_fake_state() {
        let mut slot: ModelSlot =
            serde_json::from_value(json!({"model_id":"fixture", "model_path":"PRIVATE"})).unwrap();
        for vision in [false, true] {
            slot.mmproj_path = vision.then(|| PathBuf::from("PRIVATE-PROJECTOR"));
            let value = slot.advertise().unwrap();
            assert!(!value.to_string().contains("PRIVATE"));
            let metadata: crate::model_metadata::ModelMetadata =
                serde_json::from_value(value["model_metadata"].clone()).unwrap();
            metadata.validate().unwrap();
            assert!(metadata.supports("llm.infer"));
            assert_eq!(metadata.supports("vlm.analyze"), vision);
            assert!(metadata.lifecycle.is_none());
            assert!(metadata.resource_estimates.vram_mb.is_none());
            assert_eq!(value["modality"], if vision { "vlm" } else { "text" });
        }
    }
}

pub struct ModelServer {
    pub host: String,
    pub binary: PathBuf,
    pub slots: Vec<ModelSlot>,
    processes: SyncMutex<HashMap<String, OwnedProcess>>,
    retired: SyncMutex<Vec<(String, OwnedProcess)>>,
    exit_observed: std::sync::atomic::AtomicBool,
    termination_wait_seconds: std::sync::atomic::AtomicU64,
    residency: SyncMutex<Residency>,
    operation: Mutex<()>,
    lifecycle: watch::Sender<ModelLifecycle>,
    client: reqwest::Client,
    demand_runtime: SyncMutex<Option<std::sync::Weak<crate::model_lifecycle::ModelRuntime>>>,
}

struct OwnedProcess {
    child: Child,
    _tree: crate::process_lifetime::ProcessTree,
}
impl OwnedProcess {
    async fn terminate_until(&mut self, deadline: tokio::time::Instant) -> Result<()> {
        // One deadline covers OS tree observation and root reaping. The job
        // stays owned until both finish; cancellation still closes its handle.
        #[cfg(windows)]
        self._tree.terminate_and_observe_until(deadline).await?;
        #[cfg(not(windows))]
        {
            let _ = deadline;
            self.child.start_kill()?;
        }
        self.child.wait().await?;
        Ok(())
    }
}
// Cancelled cleanup keeps the original process/tree handles, not just a PID.
// Normal runtime shutdown can still observe them before allowing replacement.
struct PendingProcess<'a> {
    server: &'a ModelServer,
    model: String,
    process: Option<OwnedProcess>,
}
impl PendingProcess<'_> {
    fn confirmed(&mut self) {
        self.process.take();
    }
}
impl Drop for PendingProcess<'_> {
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            self.server.retire_process(self.model.clone(), process);
        }
    }
}
#[derive(Clone)]
struct SlotResidency {
    state: ModelLifecycle,
    generation: u64,
    active: u32,
    control: Option<String>,
    idle_since: Option<tokio::time::Instant>,
}
struct Residency {
    closed: bool,
    slots: HashMap<String, SlotResidency>,
}
/// Inference-only lifetime pin. A stale pin cannot reactivate a later generation.
pub(crate) struct ModelUse<'a> {
    server: &'a ModelServer,
    model: String,
    generation: u64,
}
impl Drop for ModelUse<'_> {
    fn drop(&mut self) {
        let mut gate = self.server.residency.lock().unwrap();
        if let Some(slot) = gate.slots.get_mut(&self.model) {
            if slot.generation == self.generation && slot.active > 0 {
                slot.active -= 1;
                if slot.active == 0 && slot.state == ModelLifecycle::Active {
                    slot.state = ModelLifecycle::WarmIdle;
                    slot.idle_since = Some(tokio::time::Instant::now());
                }
            }
        }
        drop(gate);
        self.server.changed();
    }
}

impl ModelServer {
    pub(crate) fn bind_demand_runtime(
        &self,
        runtime: std::sync::Weak<crate::model_lifecycle::ModelRuntime>,
    ) {
        if let Some(applied) = runtime.upgrade() {
            self.termination_wait_seconds.store(
                applied.policy().termination_wait_seconds,
                std::sync::atomic::Ordering::SeqCst,
            );
        }
        *self.demand_runtime.lock().unwrap() = Some(runtime);
        self.changed();
    }
    fn demand_authorized(&self) -> bool {
        self.demand_runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|r| r.upgrade())
            .is_some_and(|r| r.demand_authorized())
    }
    fn demand_artifacts_present(&self, slot: &ModelSlot) -> bool {
        self.binary.is_file()
            && slot.model_path.is_file()
            && slot.mmproj_path.as_ref().is_none_or(|path| path.is_file())
    }
    /// Cold availability is distinct from resident readiness. Only opt-in,
    /// current runtime-owned demand loads can keep admitted requests waiting.
    pub fn can_infer(&self, model: Option<&str>) -> bool {
        if self.model_ready(model) {
            return true;
        }
        if !self.demand_authorized() {
            return false;
        }
        let Some(slot) = self.slot(model) else {
            return false;
        };
        if !self.demand_artifacts_present(slot) {
            return false;
        }
        let gate = self.residency.lock().unwrap();
        !gate.closed
            && gate.slots.get(&slot.model_id).is_some_and(|s| {
                (s.state == ModelLifecycle::Unloaded && s.control.is_none())
                    || (s.state == ModelLifecycle::Loading
                        && s.control
                            .as_deref()
                            .is_some_and(|id| id.starts_with("demand:")))
            })
    }
    #[cfg(test)]
    pub(crate) fn test_resident() -> Arc<Self> {
        advertisement_tests::resident_server()
    }
    #[cfg(test)]
    fn test_spawn_resident(&self, model: &str) -> Result<()> {
        self.exit_observed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "model_server::advertisement_tests::resident_child_helper",
                "--skip",
                "eef-owned-model-lifecycle-test",
                "--test-threads=1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (child, tree) = crate::process_lifetime::spawn_with_job_memory_limit(
            &mut command,
            self.slot(Some(model))
                .context("Test model selection missing")?
                .job_memory_limit_mib,
        )?;
        self.processes
            .lock()
            .unwrap()
            .insert(model.to_owned(), OwnedProcess { child, _tree: tree });
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_age_idle(&self, age: Duration) {
        for state in self.residency.lock().unwrap().slots.values_mut() {
            state.idle_since = Some(tokio::time::Instant::now() - age);
        }
    }
    pub fn new(binary: PathBuf, slots: Vec<ModelSlot>) -> Arc<Self> {
        Arc::new(Self {
            exit_observed: std::sync::atomic::AtomicBool::new(false),
            termination_wait_seconds: std::sync::atomic::AtomicU64::new(
                crate::model_lifecycle_policy::ModelLifecyclePolicy::default()
                    .termination_wait_seconds,
            ),
            retired: SyncMutex::new(Vec::new()),
            demand_runtime: SyncMutex::new(None),
            host: "127.0.0.1".into(),
            binary,
            slots,
            processes: SyncMutex::new(HashMap::new()),
            residency: SyncMutex::new(Residency {
                closed: false,
                slots: HashMap::new(),
            }),
            operation: Mutex::new(()),
            lifecycle: watch::channel(ModelLifecycle::Unloaded).0,
            client: reqwest::Client::new(),
        })
    }

    fn initialize_residency(&self) -> Result<()> {
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed; no new load or inference")
        }
        if self.slots.len() > 256 {
            bail!("selected local model capacity exceeded")
        }
        if gate.slots.is_empty() {
            for slot in &self.slots {
                crate::model_selection::validate_id(&slot.model_id)?;
                if gate
                    .slots
                    .insert(
                        slot.model_id.clone(),
                        SlotResidency {
                            state: ModelLifecycle::Unloaded,
                            generation: 0,
                            active: 0,
                            control: None,
                            idle_since: None,
                        },
                    )
                    .is_some()
                {
                    bail!("duplicate model selection")
                }
            }
        }
        if gate.slots.len() != self.slots.len() {
            bail!("invalid model selection history")
        }
        Ok(())
    }
    fn changed(&self) {
        let gate = self.residency.lock().unwrap();
        let state = if gate.slots.values().any(|s| {
            matches!(
                s.state,
                ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
            )
        }) {
            ModelLifecycle::Ready
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::Unloading)
        {
            ModelLifecycle::Unloading
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::UnloadUnconfirmed)
        {
            ModelLifecycle::UnloadUnconfirmed
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::Loading)
        {
            ModelLifecycle::Loading
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::Error)
        {
            ModelLifecycle::Error
        } else {
            ModelLifecycle::Unloaded
        };
        self.lifecycle.send_replace(state);
    }
    fn set_all(&self, state: ModelLifecycle) {
        let mut gate = self.residency.lock().unwrap();
        let empty = gate.slots.is_empty();
        for slot in gate.slots.values_mut() {
            // Group failure/cleanup is not a new observation of a withdrawn
            // job. Preserve its fence, including when it interrupts a stop.
            slot.state = if matches!(
                slot.state,
                ModelLifecycle::Unloading | ModelLifecycle::UnloadUnconfirmed
            ) {
                ModelLifecycle::UnloadUnconfirmed
            } else {
                state
            };
            slot.generation = slot.generation.saturating_add(1);
            slot.active = 0;
            slot.control = None;
            slot.idle_since = (slot.state == ModelLifecycle::Ready).then(tokio::time::Instant::now);
        }
        drop(gate);
        if empty {
            self.lifecycle.send_replace(state);
        } else {
            self.changed();
        }
    }
    pub(crate) fn snapshot(&self) -> Result<Vec<Value>> {
        self.initialize_residency()?;
        self.release_observed_errors();
        let gate = self.residency.lock().unwrap();
        let processes = self.processes.lock().unwrap();
        let retired = self.retired.lock().unwrap();
        let mut records = self
            .slots
            .iter()
            .map(|slot| {
                let state = &gate.slots[&slot.model_id];
                let owned = processes.get(&slot.model_id);
                let accounting = owned
                    .map(|process| process._tree.accounting())
                    .unwrap_or_else(|| {
                        if matches!(
                            state.state,
                            ModelLifecycle::Unloading | ModelLifecycle::UnloadUnconfirmed
                        ) || retired.iter().any(|(model, _)| model == &slot.model_id)
                        {
                            // The termination future owns the withdrawn job;
                            // absence from the resident map is not zero usage.
                            crate::job_accounting::JobAccounting::Unavailable {}
                        } else {
                            crate::job_accounting::JobAccounting::NoOwnedJob {}
                        }
                    });
                json!({
                    "model_id": slot.model_id,
                    "state": state.state,
                    "generation": state.generation,
                    "active_requests": state.active,
                    "control_pending": state.control.is_some(),
                    "owned_pid": owned.and_then(|process| process.child.id()),
                    "job_accounting": accounting,
                })
            })
            .collect::<Vec<_>>();
        records.sort_by(|a, b| a["model_id"].as_str().cmp(&b["model_id"].as_str()));
        Ok(records)
    }
    pub fn model_ready(&self, model: Option<&str>) -> bool {
        if self.initialize_residency().is_err() {
            return false;
        }
        let Some(selected) = self.slot(model) else {
            return false;
        };
        let gate = self.residency.lock().unwrap();
        let ready = gate.slots.get(&selected.model_id).is_some_and(|s| {
            s.control.is_none()
                && matches!(
                    s.state,
                    ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
                )
        });
        ready
            && self
                .processes
                .lock()
                .unwrap()
                .get_mut(&selected.model_id)
                .is_some_and(|p| p.child.try_wait().is_ok_and(|status| status.is_none()))
    }
    pub(crate) fn pin(&self, model: Option<&str>) -> Result<ModelUse<'_>> {
        self.initialize_residency()?;
        let selected = self.slot(model).context("Selected local model missing")?;
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed")
        }
        let s = gate.slots.get_mut(&selected.model_id).unwrap();
        if s.control.is_some()
            || !matches!(
                s.state,
                ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
            )
        {
            bail!("model_not_ready: selected model not ready")
        }
        if !self
            .processes
            .lock()
            .unwrap()
            .get_mut(&selected.model_id)
            .is_some_and(|p| p.child.try_wait().is_ok_and(|status| status.is_none()))
        {
            bail!("model_not_ready: owned process missing/exited")
        }
        s.active = s
            .active
            .checked_add(1)
            .context("model pin capacity exceeded")?;
        s.state = ModelLifecycle::Active;
        s.idle_since = None;
        let generation = s.generation;
        drop(gate);
        self.changed();
        Ok(ModelUse {
            server: self,
            model: selected.model_id.clone(),
            generation,
        })
    }
    pub(crate) fn reserve_control(
        &self,
        model: &str,
        generation: u64,
        id: &str,
        load: bool,
    ) -> Result<()> {
        self.initialize_residency()?;
        self.release_observed_errors();
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed")
        }
        let s = gate.slots.get_mut(model).context("model not selected")?;
        if s.state == ModelLifecycle::UnloadUnconfirmed {
            bail!(
                "previous model unload was not confirmed; inspect its receipt and restart the node before another load"
            )
        }
        if load
            && self
                .retired
                .lock()
                .unwrap()
                .iter()
                .any(|(name, _)| name == model)
        {
            bail!(
                "previous owned model exit is not confirmed; inspect lifecycle before another load"
            )
        }
        if s.generation != generation
            || s.active != 0
            || s.control.is_some()
            || matches!(s.state, ModelLifecycle::Loading | ModelLifecycle::Unloading)
        {
            bail!("model generation changed, busy or in use")
        }
        if load && self.processes.lock().unwrap().contains_key(model) {
            bail!("previous owned model is still resident; inspect lifecycle before another load")
        }
        if load && !matches!(s.state, ModelLifecycle::Unloaded | ModelLifecycle::Error) {
            bail!("model already resident; inspect exact receipt")
        }
        if !load && !matches!(s.state, ModelLifecycle::Ready | ModelLifecycle::WarmIdle) {
            bail!("model not resident")
        }
        s.control = Some(id.into());
        drop(gate);
        self.changed();
        Ok(())
    }
    pub(crate) fn release_control(&self, model: &str, id: &str) {
        let mut gate = self.residency.lock().unwrap();
        if let Some(s) = gate.slots.get_mut(model) {
            if s.control.as_deref() == Some(id) {
                s.control = None;
            }
        }
        drop(gate);
        self.changed();
    }
    pub(crate) fn idle_candidates(&self, idle: Duration) -> Result<Vec<(String, u64)>> {
        self.initialize_residency()?;
        let gate = self.residency.lock().unwrap();
        let mut rows = gate
            .slots
            .iter()
            .filter(|(_, s)| Self::idle_eligible(s, idle))
            .map(|(id, s)| (id.clone(), s.generation))
            .collect::<Vec<_>>();
        rows.sort();
        Ok(rows)
    }
    fn idle_eligible(slot: &SlotResidency, idle: Duration) -> bool {
        slot.active == 0
            && slot.control.is_none()
            && matches!(slot.state, ModelLifecycle::Ready | ModelLifecycle::WarmIdle)
            && slot.idle_since.is_some_and(|since| since.elapsed() >= idle)
    }
    /// The elapsed check and control reservation share the inference-pin gate.
    /// A candidate snapshot alone never authorizes unloading a recently used model.
    pub(crate) fn reserve_idle(
        &self,
        model: &str,
        generation: u64,
        idle: Duration,
        id: &str,
    ) -> Result<()> {
        self.initialize_residency()?;
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed")
        }
        let state = gate.slots.get_mut(model).context("idle model missing")?;
        if state.generation != generation || !Self::idle_eligible(state, idle) {
            bail!("idle candidate changed, pending or in use")
        }
        state.control = Some(id.into());
        drop(gate);
        self.changed();
        Ok(())
    }
    pub(crate) fn close(&self) {
        let mut gate = self.residency.lock().unwrap();
        gate.closed = true;
        self.retire_children();
        let retired = self.retired.lock().unwrap();
        for (model, s) in &mut gate.slots {
            s.generation = s.generation.saturating_add(1);
            s.state = if matches!(
                s.state,
                ModelLifecycle::Unloading | ModelLifecycle::UnloadUnconfirmed
            ) || retired.iter().any(|(name, _)| name == model)
            {
                ModelLifecycle::UnloadUnconfirmed
            } else {
                ModelLifecycle::Unloaded
            };
            s.active = 0;
            s.control = None;
            s.idle_since = None;
        }
        drop(retired);
        drop(gate);
        self.changed();
    }

    pub async fn start(&self, health_timeout: Duration) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("model operation already running")?;
        self.initialize_residency()?;
        self.release_observed_errors();
        {
            let gate = self.residency.lock().unwrap();
            if gate
                .slots
                .values()
                .any(|s| s.state == ModelLifecycle::UnloadUnconfirmed)
            {
                bail!("previous model unload was not confirmed; no startup retry in this runtime")
            }
            if !self.retired.lock().unwrap().is_empty() {
                bail!("previous owned model exit is not confirmed; no startup retry")
            }
            if gate.slots.values().any(|s| {
                s.active > 0
                    || s.control.is_some()
                    || matches!(
                        s.state,
                        ModelLifecycle::Unloading | ModelLifecycle::UnloadUnconfirmed
                    )
            }) {
                bail!("explicit model control/inference already owns startup")
            }
        }
        if *self.lifecycle.borrow() == ModelLifecycle::Ready {
            bail!("model group already started");
        }
        self.set_all(ModelLifecycle::Loading);
        // Dropping a startup future (restart/shutdown) must kill its children,
        // even if another Arc still references this server.
        let mut cleanup = StartupCleanup {
            server: self,
            armed: true,
        };
        let result = tokio::time::timeout(health_timeout, self.start_inner())
            .await
            .context("model startup timed out")
            .and_then(|result| result);
        match &result {
            Ok(()) => {
                self.set_all(ModelLifecycle::Ready);
            }
            Err(_) => {
                let stopped = self.stop_children().await;
                self.set_all(ModelLifecycle::Error);
                if let Err(error) = stopped {
                    cleanup.armed = false;
                    return Err(error)
                        .context("startup failed and owned model termination is unconfirmed");
                }
            }
        }
        cleanup.armed = false;
        result
    }

    async fn start_inner(&self) -> Result<()> {
        if self.slots.is_empty() {
            return Ok(());
        }
        if !self.binary.is_file() {
            bail!(
                "llama-server binary '{}' does not exist",
                self.binary.display()
            );
        }
        for slot in &self.slots {
            self.spawn_slot(slot, None)
                .with_context(|| format!("spawn model '{}'", slot.model_id))?;
        }
        for slot in &self.slots {
            loop {
                self.check_children()?;
                if self.healthy(slot).await {
                    break;
                }
                sleep(Duration::from_millis(250)).await;
            }
        }
        self.check_children()?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + self.termination_timeout();
        tokio::time::timeout_at(deadline, async {
            let _operation = self.operation.lock().await;
            self.stop_children_until(deadline, std::future::ready(Ok(())))
                .await?;
            self.set_all(ModelLifecycle::Unloaded);
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("model group stop admission/observation timed out")?
    }

    fn retire_process(&self, model: String, mut process: OwnedProcess) {
        self.exit_observed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        #[cfg(windows)]
        let _ = process._tree.request_termination();
        let _ = process.child.start_kill();
        self.retired.lock().unwrap().push((model, process));
    }
    fn retire_children(&self) {
        let processes = std::mem::take(&mut *self.processes.lock().unwrap());
        for (model, process) in processes {
            self.retire_process(model, process);
        }
    }
    fn release_observed_errors(&self) {
        let gate = self.residency.lock().unwrap();
        if gate.closed {
            return;
        }
        let mut retired = self.retired.lock().unwrap();
        retired.retain_mut(|(model, process)| {
            // An old failed unload remains fenced until a fresh runtime. Only
            // ordinary startup/load errors can be explicitly retried in place.
            if !gate
                .slots
                .get(model)
                .is_some_and(|slot| slot.state == ModelLifecycle::Error)
            {
                return true;
            }
            let root_exited = process
                .child
                .try_wait()
                .is_ok_and(|status| status.is_some());
            #[cfg(windows)]
            let tree_empty = matches!(
                process._tree.accounting(),
                crate::job_accounting::JobAccounting::Available {
                    active_processes: 0,
                    ..
                }
            );
            #[cfg(not(windows))]
            let tree_empty = true;
            !(root_exited && tree_empty)
        });
    }
    fn termination_timeout(&self) -> Duration {
        Duration::from_secs(
            self.termination_wait_seconds
                .load(std::sync::atomic::Ordering::SeqCst),
        )
    }
    async fn stop_children(&self) -> Result<()> {
        self.stop_children_checked(self.termination_timeout(), std::future::ready(Ok(())))
            .await
    }
    async fn stop_children_checked(
        &self,
        timeout: Duration,
        before_observation: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        if timeout.is_zero() || timeout > Duration::from_secs(30) {
            bail!("invalid model group termination wait")
        }
        let deadline = tokio::time::Instant::now() + timeout;
        self.stop_children_until(deadline, before_observation).await
    }
    async fn stop_children_until(
        &self,
        deadline: tokio::time::Instant,
        before_observation: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        if tokio::time::Instant::now() >= deadline {
            bail!("model group stop deadline expired")
        }
        let id = format!("stop:{}", uuid::Uuid::new_v4());
        let (mut pending, fences, missing) = {
            let mut gate = self.residency.lock().unwrap();
            // Transfer ownership and withdraw the same slots atomically with
            // status/admission. An empty map between these steps is not exit.
            self.retire_children();
            let processes = std::mem::take(&mut *self.retired.lock().unwrap());
            let pending = processes
                .into_iter()
                .map(|(model, process)| PendingProcess {
                    server: self,
                    model,
                    process: Some(process),
                })
                .collect::<Vec<_>>();
            let mut fences = Vec::new();
            let mut missing = false;
            for (model, slot) in &mut gate.slots {
                if !pending.iter().any(|process| &process.model == model) {
                    missing |= slot.state == ModelLifecycle::UnloadUnconfirmed
                        && !self.exit_observed.load(std::sync::atomic::Ordering::SeqCst);
                    continue;
                }
                let sticky = slot.state == ModelLifecycle::UnloadUnconfirmed;
                slot.state = ModelLifecycle::Unloading;
                slot.generation = slot.generation.saturating_add(1);
                slot.active = 0;
                slot.idle_since = None;
                slot.control = Some(id.clone());
                fences.push((model.clone(), slot.generation, sticky));
            }
            (pending, fences, missing)
        };
        let mut cleanup = GroupStopCleanup {
            server: self,
            id,
            fences,
            armed: true,
        };
        self.changed();
        tokio::time::timeout_at(deadline, async {
            before_observation.await?;
            let outcomes = futures_util::future::join_all(
                pending
                    .iter_mut()
                    .map(|process| process.process.as_mut().unwrap().terminate_until(deadline)),
            )
            .await;
            for outcome in outcomes {
                outcome?;
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("model group observation completed after deadline")
            }
            if missing {
                bail!("model termination evidence unavailable")
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("model group termination observation timed out")??;
        cleanup.finish()?;
        for process in &mut pending {
            process.confirmed();
        }
        self.exit_observed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn check_children(&self) -> Result<()> {
        let gate = self.residency.lock().unwrap();
        let mut processes = self.processes.lock().unwrap();
        let expected = gate
            .slots
            .values()
            .filter(|s| {
                matches!(
                    s.state,
                    ModelLifecycle::Loading
                        | ModelLifecycle::Ready
                        | ModelLifecycle::Active
                        | ModelLifecycle::WarmIdle
                )
            })
            .count();
        if processes.len() != expected {
            bail!("model group does not own every configured process");
        }
        for process in processes.values_mut() {
            if process.child.try_wait()?.is_some() {
                bail!("model process exited");
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) async fn load_reserved(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        spawn_authorized: impl FnOnce(&ModelSlot) -> Result<()>,
    ) -> Result<()> {
        self.load_reserved_checked(model, id, timeout, spawn_authorized, || Ok(()))
            .await
    }
    pub(crate) async fn load_reserved_checked<G>(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        spawn_authorized: impl FnOnce(&ModelSlot) -> Result<()>,
        complete_authorized: impl FnOnce() -> Result<G>,
    ) -> Result<()> {
        let _operation = self.operation.lock().await;
        self.load_reserved_inner(
            model,
            id,
            timeout,
            spawn_authorized,
            complete_authorized,
            false,
        )
        .await
        .map(|_| ())
    }
    async fn load_reserved_inner<G>(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        spawn_authorized: impl FnOnce(&ModelSlot) -> Result<()>,
        complete_authorized: impl FnOnce() -> Result<G>,
        pin: bool,
    ) -> Result<Option<ModelUse<'_>>> {
        let slot = self.slot(Some(model)).context("selected model missing")?;
        {
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed")
            }
            let s = gate.slots.get_mut(model).context("model state missing")?;
            if s.control.as_deref() != Some(id)
                || s.active != 0
                || !matches!(s.state, ModelLifecycle::Unloaded | ModelLifecycle::Error)
            {
                bail!("reserved model state changed")
            }
            let generation = s
                .generation
                .checked_add(1)
                .context("model generation exhausted")?;
            s.state = ModelLifecycle::Loading;
            s.idle_since = None;
            s.generation = generation;
        }
        self.changed();
        let mut cleanup = SlotStartupCleanup {
            server: self,
            model: model.into(),
            id: id.into(),
            armed: true,
        };
        let outcome = tokio::time::timeout(timeout, async {
            spawn_authorized(slot)?;
            loop {
                {
                    let gate = self.residency.lock().unwrap();
                    if gate.closed {
                        bail!("model runtime closed")
                    }
                    let mut processes = self.processes.lock().unwrap();
                    let p = processes.get_mut(model).context("owned model missing")?;
                    if p.child.try_wait()?.is_some() {
                        bail!("owned model exited during load")
                    }
                }
                if self.healthy(slot).await {
                    break;
                }
                sleep(Duration::from_millis(250)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("model load timed out")
        .and_then(|r| r);
        if outcome.is_ok() {
            let _authority = complete_authorized()?;
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed before load confirmation")
            }
            {
                let mut processes = self.processes.lock().unwrap();
                let owned = processes
                    .get_mut(model)
                    .context("owned model missing before readiness handoff")?;
                if owned.child.try_wait()?.is_some() {
                    bail!("owned model exited before readiness handoff")
                }
            }
            let s = gate.slots.get_mut(model).unwrap();
            if s.control.as_deref() != Some(id) {
                bail!("model load ownership changed")
            }
            s.state = if pin {
                ModelLifecycle::Active
            } else {
                ModelLifecycle::Ready
            };
            s.active = if pin { 1 } else { 0 };
            s.idle_since = (!pin).then(tokio::time::Instant::now);
            s.control = None;
            let generation = s.generation;
            cleanup.armed = false;
            drop(gate);
            self.changed();
            return Ok(pin.then(|| ModelUse {
                server: self,
                model: model.to_owned(),
                generation,
            }));
        }
        outcome.map(|_| None)
    }
    pub(crate) async fn acquire_demand_pin<G>(
        &self,
        model: &str,
        timeout: Duration,
        spawn_authorized: impl FnOnce(&ModelSlot, &str) -> Result<()>,
        authorize: impl Fn() -> Result<G>,
    ) -> Result<ModelUse<'_>> {
        if !self.can_infer(Some(model)) {
            bail!("model unavailable; no demand retry or fallback")
        }
        let _operation = self.operation.lock().await;
        {
            let _authority = authorize()?;
            if self.model_ready(Some(model)) {
                return self.pin(Some(model));
            }
            // An earlier cancelled/failed load is Error, not a fresh cold start.
            if !self.can_infer(Some(model)) {
                bail!("demand model state changed; no retry")
            }
        }
        let generation = self
            .snapshot()?
            .iter()
            .find(|s| s["model_id"] == model)
            .and_then(|s| s["generation"].as_u64())
            .context("demand model state missing")?;
        let id = format!("demand:{}", uuid::Uuid::new_v4());
        self.reserve_control(model, generation, &id, true)?;
        self.load_reserved_inner(
            model,
            &id,
            timeout,
            |slot| spawn_authorized(slot, &id),
            authorize,
            true,
        )
        .await?
        .context("demand load did not transfer its inference pin")
    }
    #[cfg(test)]
    pub(crate) async fn unload_reserved<G>(
        &self,
        model: &str,
        id: &str,
        authorize: impl FnOnce() -> Result<G>,
    ) -> Result<()> {
        self.unload_reserved_with_timeout(model, id, Duration::from_secs(5), authorize)
            .await
    }
    pub(crate) async fn unload_reserved_with_timeout<G>(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        authorize: impl FnOnce() -> Result<G>,
    ) -> Result<()> {
        self.unload_reserved_checked(model, id, timeout, authorize, |process, deadline| {
            Box::pin(process.terminate_until(deadline))
        })
        .await
    }
    async fn unload_reserved_checked<G, F>(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        authorize: impl FnOnce() -> Result<G>,
        terminate: F,
    ) -> Result<()>
    where
        F: for<'p> FnOnce(
            &'p mut OwnedProcess,
            tokio::time::Instant,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<()>> + Send + 'p>,
        >,
    {
        if timeout.is_zero() || timeout > Duration::from_secs(30) {
            bail!("invalid model termination wait")
        }
        let _operation = self.operation.lock().await;
        let (generation, process) = {
            // Acquire current authority only after asynchronous operation admission.
            // Retain its guard through withdrawal, never across the child wait.
            let _authority = authorize()?;
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed")
            }
            let s = gate.slots.get_mut(model).context("model state missing")?;
            if s.control.as_deref() != Some(id)
                || s.active != 0
                || !matches!(s.state, ModelLifecycle::Ready | ModelLifecycle::WarmIdle)
            {
                bail!("reserved model unload state changed")
            }
            let generation = s
                .generation
                .checked_add(1)
                .context("model generation exhausted")?;
            s.state = ModelLifecycle::Unloading;
            s.idle_since = None;
            s.generation = generation;
            (generation, self.processes.lock().unwrap().remove(model))
        };
        self.changed();
        let mut cleanup = SlotUnloadCleanup {
            server: self,
            model: model.into(),
            id: id.into(),
            generation,
            armed: true,
        };
        let process = process.context("reserved model has no owned process to observe")?;
        let mut pending = PendingProcess {
            server: self,
            model: model.into(),
            process: Some(process),
        };
        let deadline = tokio::time::Instant::now() + timeout;
        let outcome = tokio::time::timeout_at(
            deadline,
            terminate(pending.process.as_mut().unwrap(), deadline),
        )
        .await
        .context("owned model termination observation timed out")
        .and_then(|result| result)
        .and_then(|()| {
            if tokio::time::Instant::now() >= deadline {
                bail!("owned model termination observation completed after deadline")
            }
            Ok(())
        });
        cleanup.finish(outcome.is_ok())?;
        if outcome.is_ok() {
            pending.confirmed();
        }
        outcome
    }
    pub(crate) fn spawn_slot(&self, slot: &ModelSlot, control: Option<&str>) -> Result<()> {
        slot.selection_metadata()?;
        if !self.binary.is_file() || !slot.model_path.is_file() {
            bail!("configured model runtime/artifact unavailable")
        }
        let mut command = Command::new(&self.binary);
        command
            .arg("--model")
            .arg(&slot.model_path)
            .arg("--host")
            .arg(&self.host)
            .arg("--port")
            .arg(slot.port.to_string())
            .arg("-c")
            .arg(slot.context.to_string())
            .arg("--alias")
            .arg(&slot.model_id)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(mmproj) = &slot.mmproj_path {
            if !mmproj.is_file() {
                bail!("configured projector unavailable")
            }
            command.arg("--mmproj").arg(mmproj);
        }
        configure_cpu_threads(&mut command, slot.cpu_threads)?;
        command.arg("-ngl").arg(slot.gpu_layers.to_string());
        let gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed before spawn")
        }
        let state = gate
            .slots
            .get(&slot.model_id)
            .context("model state missing before spawn")?;
        if state.state != ModelLifecycle::Loading || state.control.as_deref() != control {
            bail!("model load ownership withdrawn before spawn")
        }
        let mut owned = self.processes.lock().unwrap();
        if owned.contains_key(&slot.model_id) {
            bail!("owned model already present")
        }
        let (child, tree) = crate::process_lifetime::spawn_with_job_memory_limit(
            &mut command,
            slot.job_memory_limit_mib,
        )?;
        self.exit_observed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        owned.insert(slot.model_id.clone(), OwnedProcess { child, _tree: tree });
        Ok(())
    }

    /// Observe owned process exits after startup, without restarting or replaying
    /// work. This does not probe ongoing HTTP health or prove inference works.
    /// The caller owns this future as part of the runtime, never a detached task.
    /// Check and cleanup share operation admission so a delayed monitor cannot
    /// destroy a replacement loaded while it was waiting.
    pub async fn monitor_processes(&self) -> Result<()> {
        loop {
            {
                let _operation = self.operation.lock().await;
                if *self.lifecycle.borrow() != ModelLifecycle::Ready {
                    return Ok(());
                }
                let outcome = {
                    let gate = self.residency.lock().unwrap();
                    let mut owned = self.processes.lock().unwrap();
                    gate.slots
                        .iter()
                        .filter(|(_, s)| {
                            matches!(
                                s.state,
                                ModelLifecycle::Ready
                                    | ModelLifecycle::Active
                                    | ModelLifecycle::WarmIdle
                            )
                        })
                        .try_for_each(|(id, _)| -> Result<()> {
                            let p = owned
                                .get_mut(id)
                                .context("ready model has no owned process")?;
                            if p.child.try_wait()?.is_some() {
                                bail!("ready model process exited")
                            };
                            Ok(())
                        })
                };
                if let Err(error) = outcome {
                    // Withdraw routing before asynchronous sibling cleanup.
                    self.set_all(ModelLifecycle::Error);
                    let stopped = self.stop_children().await;
                    self.set_all(ModelLifecycle::Error);
                    stopped.context("failed model group termination is unconfirmed")?;
                    return Err(error);
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<ModelLifecycle> {
        self.lifecycle.subscribe()
    }

    pub fn require_ready(&self) -> Result<()> {
        if *self.lifecycle.borrow() != ModelLifecycle::Ready {
            bail!("model_not_ready: local model group is not ready; inspect model status");
        }
        Ok(())
    }

    pub fn models(&self) -> Result<Vec<Value>> {
        // Invalid selections remain unroutable; do not poison node control.
        let _ = self.initialize_residency();
        self.release_observed_errors();
        let demand = self.demand_authorized();
        let gate = self.residency.lock().unwrap();
        self.slots
            .iter()
            // Invalid optional selection hints fail startup, but must never
            // poison the node's registration snapshot or command connection.
            .filter(|slot| slot.selection_metadata().is_ok())
            .map(|slot| {
                let mut advertised = slot.advertise()?;
                let mut metadata = slot.selection_metadata()?;
                let (state, busy) = gate
                    .slots
                    .get(&slot.model_id)
                    .map(|s| (s.state, s.control.is_some() || gate.closed))
                    .unwrap_or((ModelLifecycle::Error, true));
                // Keep the registration vocabulary compatible with existing
                // coordinators. Exact stop observations live in owner status.
                metadata.lifecycle = Some(match state {
                    ModelLifecycle::Unloading => ModelLifecycle::Loading,
                    ModelLifecycle::UnloadUnconfirmed => ModelLifecycle::Error,
                    state => state,
                });
                metadata.availability = Some(
                    if !busy
                        && (matches!(
                            state,
                            ModelLifecycle::Ready
                                | ModelLifecycle::Active
                                | ModelLifecycle::WarmIdle
                        ) || (demand
                            && self.demand_artifacts_present(slot)
                            && state == ModelLifecycle::Unloaded))
                    {
                        ModelAvailability::Available
                    } else {
                        ModelAvailability::Unavailable
                    },
                );
                advertised["model_metadata"] = json!(metadata);
                Ok(advertised)
            })
            .collect()
    }

    pub fn slot(&self, model_id: Option<&str>) -> Option<&ModelSlot> {
        match model_id {
            Some(id) => self.slots.iter().find(|slot| slot.model_id == id),
            None => self.slots.first(),
        }
    }

    pub fn base_url(&self, model_id: Option<&str>) -> Option<String> {
        self.slot(model_id)
            .map(|slot| format!("http://{}:{}", self.host, slot.port))
    }

    async fn healthy(&self, slot: &ModelSlot) -> bool {
        self.client
            .get(format!("http://{}:{}/health", self.host, slot.port))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }
}

struct StartupCleanup<'a> {
    server: &'a ModelServer,
    armed: bool,
}
struct SlotStartupCleanup<'a> {
    server: &'a ModelServer,
    model: String,
    id: String,
    armed: bool,
}
struct SlotUnloadCleanup<'a> {
    server: &'a ModelServer,
    model: String,
    id: String,
    generation: u64,
    armed: bool,
}
struct GroupStopCleanup<'a> {
    server: &'a ModelServer,
    id: String,
    fences: Vec<(String, u64, bool)>,
    armed: bool,
}
impl GroupStopCleanup<'_> {
    fn finish(&mut self) -> Result<()> {
        let mut gate = self.server.residency.lock().unwrap();
        for (model, generation, _) in &self.fences {
            let slot = gate.slots.get(model).context("model stop state missing")?;
            if slot.generation != *generation
                || slot.control.as_deref() != Some(&self.id)
                || slot.state != ModelLifecycle::Unloading
            {
                bail!("model group stop ownership changed")
            }
        }
        for (model, _, sticky) in &self.fences {
            let slot = gate.slots.get_mut(model).unwrap();
            slot.state = if *sticky {
                ModelLifecycle::UnloadUnconfirmed
            } else {
                ModelLifecycle::Unloaded
            };
            slot.control = None;
        }
        self.armed = false;
        drop(gate);
        self.server.changed();
        Ok(())
    }
}
impl Drop for GroupStopCleanup<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut gate = self.server.residency.lock().unwrap();
        for (model, generation, _) in &self.fences {
            if let Some(slot) = gate.slots.get_mut(model) {
                if slot.generation == *generation && slot.control.as_deref() == Some(&self.id) {
                    slot.state = ModelLifecycle::UnloadUnconfirmed;
                    slot.control = None;
                }
            }
        }
        drop(gate);
        self.server.changed();
    }
}
impl SlotUnloadCleanup<'_> {
    fn finish(&mut self, confirmed: bool) -> Result<()> {
        let mut gate = self.server.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed during termination observation")
        }
        let slot = gate
            .slots
            .get_mut(&self.model)
            .context("model state missing")?;
        if slot.generation != self.generation
            || slot.control.as_deref() != Some(&self.id)
            || slot.state != ModelLifecycle::Unloading
        {
            bail!("model termination ownership changed")
        }
        slot.state = if confirmed {
            ModelLifecycle::Unloaded
        } else {
            ModelLifecycle::UnloadUnconfirmed
        };
        slot.control = None;
        self.armed = false;
        drop(gate);
        self.server.changed();
        Ok(())
    }
}
impl Drop for SlotUnloadCleanup<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut gate = self.server.residency.lock().unwrap();
        if let Some(slot) = gate.slots.get_mut(&self.model) {
            if slot.generation == self.generation
                && slot.control.as_deref() == Some(&self.id)
                && slot.state == ModelLifecycle::Unloading
            {
                slot.state = ModelLifecycle::UnloadUnconfirmed;
                slot.control = None;
            }
        }
        drop(gate);
        self.server.changed();
    }
}
impl Drop for SlotStartupCleanup<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut gate = self.server.residency.lock().unwrap();
        if let Some(s) = gate.slots.get_mut(&self.model) {
            if s.control.as_deref() == Some(&self.id) {
                if let Some(process) = self.server.processes.lock().unwrap().remove(&self.model) {
                    self.server.retire_process(self.model.clone(), process);
                }
                s.state = ModelLifecycle::Error;
                s.idle_since = None;
                s.control = None;
                s.generation = s.generation.saturating_add(1);
            }
        }
        drop(gate);
        self.server.changed();
    }
}
impl Drop for StartupCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Child::kill_on_drop is enabled. This is synchronous so cancellation
            // cannot leave a detached task spawning children after a restart.
            self.server.retire_children();
            let state = if self.server.retired.lock().unwrap().is_empty() {
                ModelLifecycle::Unloaded
            } else {
                ModelLifecycle::Error
            };
            self.server.set_all(state);
        }
    }
}
