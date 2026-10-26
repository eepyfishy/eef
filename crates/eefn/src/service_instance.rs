//! Distinguish a service launch from a short-lived offline command's lock.
use anyhow::{Result, bail};
use std::{fs::File, path::Path, time::Duration};
use tokio::time::Instant;

/// Lifetime ownership only, not proof of API readiness or healthy execution.
/// Keep both guards until shutdown, and release both before update handoff.
pub struct ServiceInstance {
    _service: File,
    _commands: File,
}

/// Another current service owns startup when this returns None. A legacy lock
/// alone never establishes that: offline commands also own it. Wait without
/// initializing settings, starting services or replaying a command. Persistent
/// contention (including an older runtime) is an error, not silent success.
pub async fn service_instance_lock(config: &Path) -> Result<Option<ServiceInstance>> {
    acquire(config, Duration::from_secs(5)).await
}

async fn acquire(config: &Path, wait: Duration) -> Result<Option<ServiceInstance>> {
    let Some(service) = super::named_instance_lock(config, "service.instance.lock")? else {
        return Ok(None);
    };
    let deadline = Instant::now() + wait;
    let mut reported = false;
    loop {
        if let Some(commands) = super::instance_lock(config)? {
            return Ok(Some(ServiceInstance {
                _service: service,
                _commands: commands,
            }));
        }
        if !reported {
            tracing::info!("Waiting for a local command to release startup ownership");
            reported = true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!(
                "Startup ownership is still held by a local command or older runtime; no new service was started. Wait for the command or stop the existing runtime before launching again."
            )
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(25))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn service_holds_both_locks_and_drop_releases_both() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let service = acquire(&config, Duration::ZERO).await.unwrap().unwrap();
        assert!(super::super::instance_lock(&config).unwrap().is_none());
        assert!(acquire(&config, Duration::ZERO).await.unwrap().is_none());
        drop(service);
        assert!(acquire(&config, Duration::ZERO).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn command_contention_cannot_report_an_existing_service() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let command = super::super::instance_lock(&config).unwrap().unwrap();
        assert!(acquire(&config, Duration::ZERO).await.is_err());
        // Failure must not leak the separate service guard or steal the command.
        assert!(
            super::super::named_instance_lock(&config, "service.instance.lock")
                .unwrap()
                .is_some()
        );
        assert!(super::super::instance_lock(&config).unwrap().is_none());
        drop(command);
        assert!(acquire(&config, Duration::ZERO).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn startup_waits_for_command_and_blocks_second_service_launch() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let command = super::super::instance_lock(&config).unwrap().unwrap();
        let mut starting = Box::pin(acquire(&config, Duration::from_secs(1)));
        tokio::select! {
            value = &mut starting => panic!("Command ownership was misclassified: {}", value.is_ok()),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        assert!(acquire(&config, Duration::ZERO).await.unwrap().is_none());
        drop(command);
        assert!(starting.await.unwrap().is_some());
        assert!(
            !config.exists(),
            "Lock acquisition cannot initialize settings"
        );
    }

    #[tokio::test]
    async fn cancellation_releases_startup_guard_without_releasing_command() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let command = super::super::instance_lock(&config).unwrap().unwrap();
        let mut starting = Box::pin(acquire(&config, Duration::from_secs(1)));
        tokio::select! {
            _ = &mut starting => panic!("Startup cannot acquire held command lock"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        drop(starting);
        assert!(
            super::super::named_instance_lock(&config, "service.instance.lock")
                .unwrap()
                .is_some()
        );
        assert!(super::super::instance_lock(&config).unwrap().is_none());
        drop(command);
    }

    #[tokio::test]
    async fn deadline_is_failure_not_success_or_lock_theft() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let _command = super::super::instance_lock(&config).unwrap().unwrap();
        let result = acquire(&config, Duration::from_millis(30)).await;
        assert!(result.is_err());
        assert!(super::super::instance_lock(&config).unwrap().is_none());
        assert!(
            super::super::named_instance_lock(&config, "service.instance.lock")
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn separate_configurations_do_not_block_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let _first = acquire(&dir.path().join("first.json"), Duration::ZERO)
            .await
            .unwrap()
            .unwrap();
        assert!(
            acquire(&dir.path().join("second.json"), Duration::ZERO)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn filesystem_errors_are_not_duplicate_service_success() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        std::fs::create_dir(config.with_extension("service.instance.lock")).unwrap();
        assert!(acquire(&config, Duration::ZERO).await.is_err());
        assert!(!config.exists());
    }
}
