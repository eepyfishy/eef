use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::event::EventBus;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePolicy {
    Off,
    Prompt,
    Auto,
}

impl UpdatePolicy {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "off" | "disabled" => Ok(Self::Off),
            "prompt" | "notify" | "" => Ok(Self::Prompt),
            "auto" | "automatic" => Ok(Self::Auto),
            other => bail!("unknown update policy '{other}'; use off, prompt, or auto"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UpdateState {
    pub enabled: bool,
    pub policy: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub feed_paused: Option<bool>,
    pub prerelease_blocked: Option<bool>,
    pub installed_version: Option<String>,
    pub restart_required: bool,
    pub last_checked_ms: Option<u64>,
    pub last_error: Option<String>,
}

pub struct UpdateService {
    manifest_url: Option<String>,
    policy: UpdatePolicy,
    interval: Duration,
    install_dir: PathBuf,
    state: RwLock<UpdateState>,
}

impl UpdateService {
    pub fn new(
        manifest_url: Option<String>,
        policy: UpdatePolicy,
        interval: Duration,
        install_dir: PathBuf,
    ) -> Arc<Self> {
        let manifest_url = manifest_url.filter(|value| !value.trim().is_empty());
        Arc::new(Self {
            state: RwLock::new(UpdateState {
                enabled: manifest_url.is_some() && policy != UpdatePolicy::Off,
                policy: format!("{policy:?}").to_lowercase(),
                current_version: crate::VERSION.into(),
                ..UpdateState::default()
            }),
            manifest_url,
            policy,
            interval: interval.max(Duration::from_secs(300)),
            install_dir,
        })
    }

    pub async fn state(&self) -> UpdateState {
        let mut state = self.state.read().await.clone();
        // Offline repair/rollback can select a bundle without this service.
        match eefn::updater::installed_version(&self.install_dir).and_then(|selected| {
            let restart =
                eefn::updater::restart_required(&self.install_dir, crate::VERSION, "eef")?;
            Ok((selected, restart))
        }) {
            Ok((selected, restart)) => {
                state.installed_version = selected;
                state.restart_required = restart;
            }
            Err(error) => {
                state.last_error = Some(format!("Package selection needs attention: {error}"))
            }
        }
        state
    }

    pub async fn check(&self) -> Result<eefn::updater::UpdateCheck> {
        let url = self
            .manifest_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("no update.manifest_url configured"))?;
        match eefn::updater::check(url, crate::VERSION, Duration::from_secs(30)).await {
            Ok(check) => {
                let mut state = self.state.write().await;
                state.latest_version = Some(check.latest_version.clone());
                state.update_available = check.update_available;
                state.feed_paused = Some(check.feed_paused);
                state.prerelease_blocked = Some(check.prerelease_blocked);
                state.last_checked_ms = Some(eefn::protocol::now_ms());
                state.last_error = None;
                Ok(check)
            }
            Err(error) => {
                let mut state = self.state.write().await;
                state.latest_version = None;
                state.update_available = false;
                state.feed_paused = None;
                state.prerelease_blocked = None;
                state.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub async fn apply(&self) -> Result<eefn::updater::UpdateApplied> {
        let check = self.check().await?;
        self.apply_checked(&check).await
    }

    async fn apply_checked(
        &self,
        check: &eefn::updater::UpdateCheck,
    ) -> Result<eefn::updater::UpdateApplied> {
        let url = self
            .manifest_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("no update.manifest_url configured"))?;
        let applied = eefn::updater::apply_checked_for(
            url,
            &self.install_dir,
            Duration::from_secs(180),
            "eef",
            check,
        )
        .await?;
        let mut state = self.state.write().await;
        state.installed_version = Some(applied.new_version.clone());
        state.restart_required = applied.restart;
        state.update_available = false;
        Ok(applied)
    }

    pub fn start(self: &Arc<Self>, bus: EventBus) -> Option<JoinHandle<()>> {
        if self.manifest_url.is_none() || self.policy == UpdatePolicy::Off {
            return None;
        }
        let service = self.clone();
        Some(tokio::spawn(async move {
            loop {
                match service.check().await {
                    Ok(check) if check.update_available => {
                        bus.publish("update.available", serde_json::json!({"program": "eef", "current": check.current_version, "latest": check.latest_version, "policy": service.state.read().await.policy})).await;
                        if service.policy == UpdatePolicy::Auto {
                            match service.apply_checked(&check).await {
                                Ok(applied) => {bus.publish("update.installed", serde_json::json!({"program": "eef", "version": applied.new_version, "restart_required": true})).await; return;},
                                Err(error) => bus.publish("update.failed", serde_json::json!({"program": "eef", "error": error.to_string()})).await,
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        bus.publish(
                            "update.check_failed",
                            serde_json::json!({"program": "eef", "error": error.to_string()}),
                        )
                        .await
                    }
                }
                tokio::time::sleep(service.interval).await;
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn update_status_and_apply_distinguish_paused_and_prerelease_feeds() {
        let root = tempfile::tempdir().unwrap();
        let feed = root.path().join("feed.json");
        let service = UpdateService::new(
            Some(feed.to_str().unwrap().into()),
            UpdatePolicy::Prompt,
            Duration::from_secs(300),
            root.path().into(),
        );
        let save = |value: serde_json::Value| {
            std::fs::write(&feed, serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let release = |version: &str| json!({"version":version,"url":"https://example.invalid/no-artifact","sha256":"0".repeat(64)});
        save(release("99.0.0"));
        let observed = service.check().await.unwrap();
        assert!(observed.update_available);
        assert!(service.state().await.update_available);
        save(release("99.0.1"));
        assert!(
            service
                .apply_checked(&observed)
                .await
                .unwrap_err()
                .to_string()
                .contains("offer changed")
        );
        save(json!({"schema_version":1,"enabled":false,"reason":"Fixture feed"}));
        assert!(
            service
                .apply()
                .await
                .unwrap_err()
                .to_string()
                .contains("paused")
        );
        let paused = service.state().await;
        assert_eq!(paused.feed_paused, Some(true));
        assert!(!paused.update_available);
        save(release("99.0.0-alpha.1"));
        assert!(
            service
                .apply()
                .await
                .unwrap_err()
                .to_string()
                .contains("offers a prerelease")
        );
        let blocked = service.state().await;
        assert_eq!(blocked.feed_paused, Some(false));
        assert_eq!(blocked.prerelease_blocked, Some(true));
        assert!(!blocked.update_available);
        save(json!({}));
        assert!(service.check().await.is_err());
        let failed = service.state().await;
        assert_eq!(failed.latest_version, None);
        assert_eq!(failed.feed_paused, None);
        assert_eq!(failed.prerelease_blocked, None);
        assert!(!failed.update_available);
        assert!(!root.path().join("versions").exists());
    }
}
