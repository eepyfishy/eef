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
        self.state.read().await.clone()
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
                state.last_checked_ms = Some(eefn::protocol::now_ms());
                state.last_error = None;
                Ok(check)
            }
            Err(error) => {
                self.state.write().await.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub async fn apply(&self) -> Result<eefn::updater::UpdateApplied> {
        if !self.check().await?.update_available {
            bail!("You are already up to date")
        }
        let url = self
            .manifest_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("no update.manifest_url configured"))?;
        let applied =
            eefn::updater::apply_for(url, &self.install_dir, Duration::from_secs(180), "eef")
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
                            match service.apply().await {
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
