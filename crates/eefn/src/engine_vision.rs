//! Node-local vision model transport, not workload/grant admission. The caller
//! must persist its analysis intent and supervise current lease/stop authority.
use super::NodeEngine;
use crate::{grants::GrantLimits, model_selection::Backend, python::vision::PrivateJpeg};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::IpAddr, time::Duration};
use tokio::time::{Instant, timeout_at};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    pub backend: Backend,
    pub model_id: String,
    /// Ollama's installed-model report, not an independent file measurement.
    pub installed_digest: Option<String>,
}
impl ModelSelection {
    pub fn validate(&self) -> Result<()> {
        crate::model_selection::validate_id(&self.model_id)?;
        ensure!(
            match self.backend {
                Backend::Ollama => self
                    .installed_digest
                    .as_deref()
                    .is_some_and(crate::model_lifecycle::valid_provider_digest),
                Backend::Llamacpp => self.installed_digest.is_none(),
            },
            "Invalid local vision model identity"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisLimits {
    pub max_tokens: u64,
    pub max_response_bytes: usize,
    pub max_result_bytes: usize,
    pub timeout_ms: u64,
}
impl AnalysisLimits {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=1024).contains(&self.max_tokens)
                && (1024..=64 * 1024).contains(&self.max_response_bytes)
                && (1..=4096).contains(&self.max_result_bytes)
                && self.max_result_bytes <= self.max_response_bytes
                && (1..=crate::grants::MAX_DURATION_MS).contains(&self.timeout_ms),
            "Invalid local vision analysis limits"
        );
        Ok(())
    }
}

/// Literal loopback only: no DNS, credentials, paths or proxy interpretation.
fn local_base(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).context("Invalid local vision endpoint")?;
    let local = url
        .host_str()
        .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    ensure!(
        raw.trim() == raw
            && local
            && matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && matches!(url.path(), "" | "/")
            && url.query().is_none()
            && url.fragment().is_none()
            && url.port_or_known_default().is_some_and(|port| port > 0),
        "Vision requires an applied literal loopback model endpoint"
    );
    Ok(url)
}

/// Borrowed from the exact applied engine; never constructed from a network URL.
/// This pins transport selection, not authorization or provider trustworthiness.
pub(crate) struct LocalBinding<'a> {
    engine: &'a NodeEngine,
    selection: ModelSelection,
    base: reqwest::Url,
    client: reqwest::Client,
}
impl NodeEngine {
    pub(crate) fn passive_vision_models(
        &self,
        saved: &Value,
    ) -> Result<(Vec<crate::vision_sources::Model>, bool)> {
        let mut rows = Vec::new();
        for (model_id, metadata) in &self.ollama_models {
            if metadata.supports("vlm.analyze") && metadata.routable() {
                rows.push(crate::vision_sources::Model {
                    backend: Backend::Ollama,
                    model_id: model_id.clone(),
                    local_endpoint_configured: local_base(&self.ollama_url).is_ok(),
                });
            }
        }
        if let Some(server) = &self.model_server {
            for slot in &server.slots {
                let metadata = slot.selection_metadata()?;
                if metadata.supports("vlm.analyze") && metadata.routable() {
                    rows.push(crate::vision_sources::Model {
                        backend: Backend::Llamacpp,
                        model_id: slot.model_id.clone(),
                        local_endpoint_configured: server
                            .base_url(Some(&slot.model_id))
                            .is_some_and(|url| local_base(&url).is_ok()),
                    });
                }
            }
        }
        rows.sort_by(|a, b| (a.backend.name(), &a.model_id).cmp(&(b.backend.name(), &b.model_id)));
        Ok((rows, self.granted_model_config.as_ref() == Some(saved)))
    }
    pub(crate) fn bind_local_vision(&self, selection: &ModelSelection) -> Result<LocalBinding<'_>> {
        selection.validate()?;
        let base = match selection.backend {
            Backend::Ollama => {
                ensure!(
                    self.ollama_models
                        .get(&selection.model_id)
                        .is_some_and(|m| m.supports("vlm.analyze") && m.routable()),
                    "Selected local vision model is unavailable"
                );
                local_base(&self.ollama_url)?
            }
            Backend::Llamacpp => {
                let server = self
                    .model_server
                    .as_ref()
                    .context("Local vision backend unavailable")?;
                let slot = server
                    .slot(Some(&selection.model_id))
                    .context("Selected local vision model unavailable")?;
                let metadata = slot.selection_metadata()?;
                ensure!(
                    metadata.supports("vlm.analyze") && metadata.routable(),
                    "Selected local model does not permit vision"
                );
                local_base(
                    &server
                        .base_url(Some(&selection.model_id))
                        .context("Local vision endpoint unavailable")?,
                )?
            }
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            // Disable the locked client's default protocol-NACK retries too.
            // A durable analysis intent must correspond to one exchange.
            .retry(reqwest::retry::never())
            .build()?;
        Ok(LocalBinding {
            engine: self,
            selection: selection.clone(),
            base,
            client,
        })
    }
}

async fn bounded_response(mut response: reqwest::Response, limit: usize) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "Local vision provider refused request"
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= limit as u64),
        "Local vision response exceeds limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit.saturating_sub(bytes.len()),
            "Local vision response exceeds limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

impl LocalBinding<'_> {
    fn current(
        &self,
        limits: &GrantLimits,
        deadline: Instant,
        authorized: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        ensure!(Instant::now() < deadline, "Local vision deadline expired");
        authorized()?;
        let params = json!({"model_id":self.selection.model_id,"backend":self.selection.backend});
        self.engine.check_operation_policy(
            crate::grants::OperationShape {
                capability: "vlm.analyze",
                action: "analyze",
                params: &params,
                resource_id: None,
                model_id: Some(&self.selection.model_id),
                backend: Some(self.selection.backend.name()),
                model_role: None,
            },
            limits,
            true,
        )?;
        ensure!(Instant::now() < deadline, "Local vision deadline expired");
        Ok(())
    }
    async fn installed(&self) -> Result<()> {
        if self.selection.backend != Backend::Ollama {
            return Ok(());
        }
        let response = self.client.get(self.base.join("api/tags")?).send().await?;
        let tags = bounded_response(response, 4 * 1024 * 1024).await?;
        let matches: Vec<_> = tags["models"]
            .as_array()
            .context("Invalid installed vision model report")?
            .iter()
            .filter(|m| m["name"].as_str() == Some(self.selection.model_id.as_str()))
            .collect();
        ensure!(
            matches.len() == 1
                && matches[0]["digest"].as_str() == self.selection.installed_digest.as_deref()
                && matches[0]["size"].as_u64().is_some_and(|n| n > 0)
                && ["remote_host", "remote_model"]
                    .iter()
                    .all(|key| matches[0].get(*key).is_none_or(|v| v.is_null() || v == "")),
            "Installed local vision model identity changed"
        );
        Ok(())
    }
    /// No retries, tools, remote fallback or automatic downloads. Dropping this
    /// future stops eef's exchange, not an independently owned model service.
    /// `authorized` checks current authority; it must not create a fresh grant.
    pub(crate) async fn analyze(
        &self,
        image: &PrivateJpeg,
        prompt: &str,
        limits: &AnalysisLimits,
        granted: &GrantLimits,
        deadline: Instant,
        authorized: impl Fn() -> Result<()> + Send + Sync,
    ) -> Result<String> {
        limits
            .validate()
            .map_err(|_| anyhow::anyhow!("Invalid local vision analysis limits"))?;
        let deadline = deadline.min(Instant::now() + Duration::from_millis(limits.timeout_ms));
        let operation = async {
            ensure!(
                !prompt.trim().is_empty()
                    && prompt.len() <= 4096
                    && !prompt.contains('\0')
                    && limits.timeout_ms <= granted.max_execution_ms
                    && limits.max_result_bytes <= granted.max_result_bytes,
                "Invalid reviewed vision call scope"
            );
            self.current(granted, deadline, &authorized)?;
            let _request_slot = self.engine.reserve_model_request()?;
            self.installed().await?;
            self.current(granted, deadline, &authorized)?;
            let model = Some(self.selection.model_id.as_str());
            let model_pin = if self.selection.backend == Backend::Llamacpp {
                let server = self
                    .engine
                    .model_server
                    .as_ref()
                    .context("Vision backend unavailable")?;
                Some(match server.pin(model) {
                    Ok(pin) => pin,
                    Err(_) => {
                        self.engine
                            .service
                            .as_ref()
                            .context("Vision model not ready")?
                            .acquire_demand_model(server, &self.selection.model_id)
                            .await?
                    }
                })
            } else {
                None
            };
            let params = json!({"model":self.selection.model_id,"backend":self.selection.backend,
                "prompt":prompt,"images":[image.as_base64()],"max_tokens":limits.max_tokens});
            let (path, payload) = match self.selection.backend {
                Backend::Ollama => ("api/chat", self.engine.ollama_payload(&params, None)?),
                Backend::Llamacpp => (
                    "v1/chat/completions",
                    self.engine.llamacpp_payload(&params, None),
                ),
            };
            self.current(granted, deadline, &authorized)?;
            let started = Instant::now();
            let response = self
                .client
                .post(self.base.join(path)?)
                .json(&payload)
                .send()
                .await?;
            let response = bounded_response(response, limits.max_response_bytes).await?;
            let exchange_time = started.elapsed();
            let result = crate::model_response::normalize(&response, self.selection.backend)?;
            ensure!(
                result["completion_complete"] == true,
                "Vision completion unconfirmed"
            );
            let text = result["content"]
                .as_str()
                .context("Vision answer missing")?;
            ensure!(
                !text.trim().is_empty()
                    && serde_json::to_vec(text)?.len() <= limits.max_result_bytes
                    && !text.contains('\0'),
                "Vision answer outside approved bounds"
            );
            self.current(granted, deadline, &authorized)?;
            self.installed().await?;
            self.current(granted, deadline, &authorized)?;
            if let Some(pin) = model_pin {
                pin.observe_decode(
                    true,
                    &response,
                    &result,
                    u64::from(limits.max_tokens),
                    exchange_time,
                );
            }
            Ok(text.to_owned())
        };
        // Provider diagnostics, URL errors and partial text never leave this
        // private helper. Durable callers retain the pre-effect intent on error.
        timeout_at(deadline, operation)
            .await
            .map_err(|_| anyhow::anyhow!("Local vision call deadline; outcome unconfirmed"))?
            .map_err(|_: anyhow::Error| {
                anyhow::anyhow!("Local vision call failed; outcome unconfirmed")
            })
    }
}

#[cfg(test)]
#[path = "engine_vision_tests.rs"]
mod tests;
