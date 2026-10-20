//! Once-admitted model-setup restarts use an immutable private configuration.
//! The snapshot is never written back over newer owner settings or serialized in reports.
use crate::{
    NodeService,
    model_bootstrap::Saved,
    model_setup::{Attempt, Phase, Reason},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Scope {
    pub id: String,
    pub origin_runtime: String,
    pub saved: Saved,
    pub models_sha256: String,
}
/// Deliberately not Debug/Serialize: contains the private configuration/credentials.
#[derive(Clone)]
pub struct StartupConfiguration {
    source: Value,
    scope: Option<Scope>,
}
impl StartupConfiguration {
    pub fn value(&self) -> &Value {
        &self.source
    }
}
struct Applied {
    runtime: String,
    source: Value,
}
#[derive(Default)]
pub(crate) struct ActivationState {
    applied: Option<Applied>,
    pending: Option<StartupConfiguration>,
    active: Option<Scope>,
}

/// Ignore setup preferences/catalog and the known live-only management consent.
/// Everything else, including opaque future settings, remains part of the fence.
fn projection(source: &Value, models: bool) -> Value {
    let mut value = source.clone();
    if let Some(map) = value.as_object_mut() {
        map.remove("model_bootstrap");
        map.remove("model_bootstrap_revision");
        map.remove("model_catalog");
        if !models {
            map.remove("models");
        }
        if let Some(management) = map.get_mut("management").and_then(Value::as_object_mut) {
            management.remove("allow_remote");
            if management.is_empty() {
                map.remove("management");
            }
        }
    }
    value
}
pub(crate) fn plan_locked(
    service: &NodeService,
    source: Value,
    attempt: &Attempt,
) -> std::result::Result<StartupConfiguration, Reason> {
    let state = service
        .activation
        .lock()
        .map_err(|_| Reason::ActivationUnavailable)?;
    let applied = state
        .applied
        .as_ref()
        .ok_or(Reason::ActivationUnavailable)?;
    let live = service.live.lock().unwrap();
    if applied.runtime != attempt.runtime_id
        || live["runtime_id"] != attempt.runtime_id
        || live["setup_model_overrides"] == true
        || live["setup_existing_applied_models"] == true
        || state.pending.is_some()
        || state.active.is_some()
    {
        return Err(Reason::ActivationUnavailable);
    }
    if projection(&applied.source, false) != projection(&source, false) {
        return Err(Reason::PendingSettings);
    }
    let scope = Scope {
        id: attempt.id.clone(),
        origin_runtime: attempt.runtime_id.clone(),
        saved: attempt.saved.clone(),
        models_sha256: crate::grants::definition_digest(&source["models"])
            .map_err(|_| Reason::ConfigurationUncertain)?,
    };
    Ok(StartupConfiguration {
        source,
        scope: Some(scope),
    })
}
pub(crate) fn enqueue_locked(service: &NodeService, startup: StartupConfiguration) -> Result<()> {
    let mut state = service
        .activation
        .lock()
        .map_err(|_| anyhow::anyhow!("Activation owner unavailable"))?;
    if state.pending.is_some() || state.active.is_some() {
        bail!("Another model activation is pending")
    }
    state.pending = Some(startup);
    service.model_activation_ready.notify_one();
    Ok(())
}
pub(crate) fn draining(service: &NodeService, id: &str, runtime: &str) -> bool {
    service.activation.lock().is_ok_and(|s| {
        s.active
            .as_ref()
            .is_some_and(|a| a.id == id && a.origin_runtime == runtime)
    })
}
impl NodeService {
    /// One configuration read per runtime; API settings and engine use the same bytes.
    pub fn capture_runtime_configuration(&self) -> Result<StartupConfiguration> {
        self.inspect_grant_configuration(|config| {
            Ok(StartupConfiguration {
                source: if self.config_path.is_file() {
                    config.clone()
                } else {
                    json!({})
                },
                scope: None,
            })
        })
    }
    /// Final admission, before retiring the old runtime. Later owner edits stay
    /// saved/pending; they cannot change this already-admitted restart's scope.
    pub fn claim_model_activation(&self) -> Result<Option<StartupConfiguration>> {
        self.inspect_grant_configuration(|current| {
            let mut state = self
                .activation
                .lock()
                .map_err(|_| anyhow::anyhow!("Activation owner unavailable"))?;
            let Some(startup) = state.pending.take() else {
                return Ok(None);
            };
            let scope = startup
                .scope
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing activation scope"))?;
            let live = self.live.lock().unwrap();
            let unchanged = state.active.is_none()
                && live["runtime_id"] == scope.origin_runtime
                && live["setup_model_overrides"] != true
                && live["setup_existing_applied_models"] != true
                && !live["models"].as_array().is_some_and(|m| !m.is_empty())
                && state.applied.as_ref().is_some_and(|a| {
                    a.runtime == scope.origin_runtime
                        && projection(&a.source, false) == projection(current, false)
                })
                && Saved::from_config(current).is_ok_and(|s| s == scope.saved)
                && scope.saved.policy.catalog.as_ref().is_some_and(|pin| {
                    crate::model_bootstrap::CatalogPin::from_config(current, &pin.model_id)
                        .is_ok_and(|now| now == *pin)
                })
                && current["node_id"] == self.node_id
                && current["models"] == startup.source["models"]
                && projection(current, false) == projection(&startup.source, false);
            drop(live);
            if !unchanged {
                crate::model_setup::activation_transition(
                    self,
                    scope,
                    Phase::ApplyBlocked,
                    Some(Reason::StateChanged),
                    None,
                )?;
                self.live.lock().unwrap()["model_activation"] =
                    json!({"state":"blocked","attempt_id":scope.id});
                return Ok(None);
            }
            crate::model_setup::activation_transition(
                self,
                scope,
                Phase::RestartDraining,
                None,
                None,
            )?;
            state.active = Some(scope.clone());
            self.live.lock().unwrap()["model_activation"] =
                json!({"state":"draining","attempt_id":scope.id});
            Ok(Some(startup))
        })
    }
    /// Publish the effective configuration atomically with its pending-settings
    /// hint. Success means engine configuration, never loading/inference/readiness.
    pub fn adopt_runtime_configuration(
        &self,
        runtime: &str,
        startup: &StartupConfiguration,
        publish: impl FnOnce(&mut Value, bool),
    ) -> Result<()> {
        crate::automation_occurrence::uuid(runtime)?;
        self.inspect_grant_configuration(|current| {
            let mut state = self
                .activation
                .lock()
                .map_err(|_| anyhow::anyhow!("Activation owner unavailable"))?;
            let notice = if let Some(scope) = &startup.scope {
                if state.active.as_ref() != Some(scope)
                    || runtime == scope.origin_runtime
                    || crate::grants::definition_digest(&startup.source["models"])?
                        != scope.models_sha256
                {
                    bail!("Scoped startup token is stale or already consumed")
                }
                let completed = crate::model_setup::activation_transition(
                    self, scope, Phase::RuntimeConfigured, None, Some(runtime),
                ).is_ok();
                if !completed {
                    let _ = crate::model_setup::activation_transition(
                        self, scope, Phase::Unknown,
                        Some(Reason::ConfigurationUncertain), Some(runtime),
                    );
                    tracing::warn!("Model activation result checkpoint unconfirmed; inspect retained setup and model lifecycle");
                }
                state.active = None;
                json!({"state":if completed {"runtime_configured"}else{"unknown"},"attempt_id":scope.id,"runtime_id":runtime,"readiness_verified":false})
            } else {
                Value::Null
            };
            let pending = projection(current, true) != projection(&startup.source, true);
            state.applied = Some(Applied {
                runtime: runtime.into(),
                source: startup.source.clone(),
            });
            let mut live = self.live.lock().unwrap();
            publish(&mut live, pending);
            live["model_activation"] = notice;
            Ok(())
        })
    }
    /// Generic restart/exit and unconfirmed shutdown never replay a setup token.
    pub fn cancel_model_activation(&self) {
        let result = self.serialize_configuration(|| {
            let mut state = self
                .activation
                .lock()
                .map_err(|_| anyhow::anyhow!("Activation owner unavailable"))?;
            let scope = state
                .active
                .take()
                .or_else(|| state.pending.take().and_then(|p| p.scope));
            state.pending = None;
            if let Some(scope) = scope {
                let _ = crate::model_setup::activation_transition(
                    self,
                    &scope,
                    Phase::Unknown,
                    Some(Reason::RuntimeInterrupted),
                    None,
                );
                self.live.lock().unwrap()["model_activation"] =
                    json!({"state":"unknown","attempt_id":scope.id});
            }
            Ok(())
        });
        if result.is_err() {
            tracing::warn!("Model activation cancellation checkpoint unavailable");
        }
    }
}
