//! Read-only, policy/disclosure-scoped selection through existing model ranking.
//! A route observation never reserves an allowance or authorizes execution.
use crate::{
    Runtime,
    model::{Modality, ModelRequest},
};
use anyhow::{Context, Result, ensure};
use eefn::{interpreter_policy::Authorization, interpreter_service::Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Options {
    #[arg(long = "origin")]
    pub origin_node: String,
    #[arg(long)]
    #[serde(default)]
    pub local_only: bool,
    #[arg(long = "provider")]
    pub node_id: Option<String>,
    #[arg(long)]
    pub backend: Option<String>,
    #[arg(long)]
    pub model_id: Option<String>,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.origin_node)?;
        if let Some(node) = &self.node_id {
            eefn::network::validate_remote_node_id(node)?;
            ensure!(
                !self.local_only || node == &self.origin_node,
                "Local-only selection conflicts with provider"
            );
        }
        ensure!(
            self.backend
                .as_ref()
                .is_none_or(|b| matches!(b.as_str(), "ollama" | "llamacpp")),
            "Unsupported interpreter backend"
        );
        if let Some(model) = &self.model_id {
            eefn::model_selection::validate_id(model)?;
        }
        Ok(())
    }
    fn model_request(&self) -> Result<ModelRequest> {
        self.validate()?;
        let mut constraints = json!({"model_role":"request_interpreter"});
        if let Some(node) = self
            .node_id
            .as_deref()
            .or(self.local_only.then_some(self.origin_node.as_str()))
        {
            constraints["node_id"] = json!(node);
        }
        if let Some(backend) = &self.backend {
            constraints["backend"] = json!(backend);
        }
        Ok(ModelRequest {
            modality: Modality::Text,
            tier: None,
            model_id: self.model_id.clone(),
            constraints,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: Options,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter route schema"
        );
        eefn::interpreter_policy::uuid(&self.expected_runtime_id)?;
        self.command.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub node_id: String,
    pub runtime_id: String,
    pub session_id: String,
}
impl Connection {
    fn from_session(session: &Session) -> Self {
        Self {
            node_id: session.node_id().into(),
            runtime_id: session.runtime_id().into(),
            session_id: session.session_id().into(),
        }
    }
    fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.node_id)?;
        eefn::interpreter_policy::uuid(&self.runtime_id)?;
        eefn::interpreter_policy::uuid(&self.session_id)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selected {
    pub connection: Connection,
    pub backend: String,
    pub model_id: String,
    pub permission_revision: u64,
    pub grant_limits: eefn::grants::GrantPolicy,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub command: Options,
    pub policy_id: String,
    pub policy_generation: u64,
    pub policy_sha256: String,
    pub issuer_id: String,
    pub origin: Connection,
    pub selected: Option<Selected>,
    pub grants_issued: bool,
    pub model_inference_requested: bool,
    pub allowance_reserved: bool,
    pub automatic_delegation_available: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, command: &Options, runtime: &str) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "interpreter_route_preview"
        || !report.success
        || report.runtime_id != runtime
        || report.command != *command
        || eefn::interpreter_policy::uuid(runtime).is_err()
        || eefn::interpreter_policy::uuid(&report.policy_id).is_err()
        || report.policy_generation == 0
        || report.policy_generation > i64::MAX as u64
        || eefn::request_interpretation::digest(&report.policy_sha256).is_err()
        || eefn::request_interpretation::digest(&report.issuer_id).is_err()
        || report.origin.validate().is_err()
        || report.origin.node_id != command.origin_node
        || report.grants_issued
        || report.model_inference_requested
        || report.allowance_reserved
        || report.automatic_delegation_available
        || report.automatically_retried
    {
        return false;
    }
    report.selected.as_ref().is_none_or(|selected| {
        selected.connection.validate().is_ok()
            && selected.grant_limits.validate().is_ok()
            && matches!(selected.backend.as_str(), "ollama" | "llamacpp")
            && eefn::model_selection::validate_id(&selected.model_id).is_ok()
            && (!command.local_only || selected.connection.node_id == command.origin_node)
            && command
                .node_id
                .as_ref()
                .is_none_or(|n| n == &selected.connection.node_id)
            && command
                .backend
                .as_ref()
                .is_none_or(|b| b == &selected.backend)
            && command
                .model_id
                .as_ref()
                .is_none_or(|m| m == &selected.model_id)
    })
}
fn current_policy(runtime: &Runtime) -> Result<Authorization> {
    runtime.config.edit_for_restart(|saved| {
        let settings = eefn::authentication::AuthenticationPolicy::parse(
            saved.pointer("/node/authentication"),
        )?;
        let issuer = crate::interpreter_policy::current_issuer(
            runtime.node_server.authentication_policy(),
            &settings,
        )?
        .context("Apply private authentication settings before resolving an interpreter")?;
        let value = runtime.engine.store.interpreter_policy_command(
            eefn::interpreter_policy::Command::Show { id: None },
            runtime.runtime_id(),
            None,
            eefn::protocol::now_ms(),
        )?;
        let record: Authorization = serde_json::from_value(value["authorization"].clone())
            .context(
                "Approve an explicit interpreter service policy before resolving a provider",
            )?;
        ensure!(
            record.active(
                value["current_generation"]
                    .as_u64()
                    .context("Missing policy generation")?,
                &issuer,
                eefn::protocol::now_ms()
            ),
            "Interpreter policy is revoked, expired, superseded or bound to another issuer"
        );
        Ok((record, false))
    })
}

pub(crate) async fn resolve(
    runtime: &Runtime,
    discovery: &crate::discovery::DiscoveryPolicy,
    request: Request,
) -> Result<Value> {
    request.validate()?;
    ensure!(
        request.expected_runtime_id == runtime.runtime_id(),
        "Coordinator runtime changed before interpreter selection"
    );
    let policy = current_policy(runtime)?;
    let eefn::interpreter_policy::Command::Approve { scope, .. } = &policy.creation else {
        unreachable!()
    };
    ensure!(
        scope.origin_nodes.contains(&request.command.origin_node),
        "Input node is outside interpreter policy"
    );
    let disclosed = discovery
        .planning_nodes(&request.command.origin_node, &runtime.node_server)
        .await?;
    let origin = runtime
        .node_server
        .interpreter_session(&request.command.origin_node, discovery.freshness_seconds)
        .await?;
    ensure!(
        origin.issuer_id() == policy.issuer_id,
        "Input issuer changed"
    );
    let mut chosen = None;
    // Ranking, explicit model/backend constraints and warm/load/latency scores
    // remain owned by the existing registry. Stale registry entries cannot pass
    // the independently authenticated registration check below.
    for candidate in runtime
        .model_registry
        .route(&request.command.model_request()?, &Default::default())
    {
        if !scope.provider_nodes.contains(&candidate.node_id)
            || !disclosed.contains(&candidate.node_id)
        {
            continue;
        }
        let Ok(provider) = runtime
            .node_server
            .interpreter_session(&candidate.node_id, discovery.freshness_seconds)
            .await
        else {
            continue;
        };
        if provider.issuer_id() != policy.issuer_id
            || !provider.serves(&candidate.backend, &candidate.model_id)
        {
            continue;
        }
        chosen = Some((provider, candidate));
        break;
    }
    let current_origin = runtime
        .node_server
        .interpreter_session(origin.node_id(), discovery.freshness_seconds)
        .await?;
    ensure!(
        origin.same_connection(&current_origin),
        "Input connection changed during interpreter selection"
    );
    let selected = if let Some((provider, model)) = chosen {
        let current = runtime
            .node_server
            .interpreter_session(provider.node_id(), discovery.freshness_seconds)
            .await?;
        ensure!(
            provider.same_connection(&current)
                && provider.permission_revision() == current.permission_revision()
                && json!(provider.limits()) == json!(current.limits())
                && current.serves(&model.backend, &model.model_id),
            "Selected interpreter changed; no replacement provider was selected"
        );
        Some(Selected {
            connection: Connection::from_session(&current),
            backend: model.backend,
            model_id: model.model_id,
            permission_revision: current
                .permission_revision()
                .context("Interpreter sharing unavailable")?,
            grant_limits: current.limits().clone(),
        })
    } else {
        None
    };
    let latest = current_policy(runtime)?;
    ensure!(
        latest.id == policy.id
            && latest.authorization_sha256 == policy.authorization_sha256
            && latest.revision == policy.revision,
        "Interpreter policy changed during selection"
    );
    let report = Report {
        schema_version: 1,
        report_type: "interpreter_route_preview".into(),
        success: true,
        runtime_id: runtime.runtime_id().into(),
        command: request.command.clone(),
        policy_id: policy.id,
        policy_generation: policy.generation,
        policy_sha256: policy.authorization_sha256,
        issuer_id: policy.issuer_id,
        origin: Connection::from_session(&current_origin),
        selected,
        grants_issued: false,
        model_inference_requested: false,
        allowance_reserved: false,
        automatic_delegation_available: false,
        automatically_retried: false,
    };
    let value = json!(report);
    ensure!(
        confirmed(&value, &request.command, runtime.runtime_id()),
        "Interpreter route report unconfirmed"
    );
    Ok(value)
}
pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    ensure!(
        confirmed(value, &report.command, &report.runtime_id),
        "Unconfirmed interpreter route"
    );
    let text = if let Some(selected) = report.selected {
        format!(
            "Advertised interpreter for node {}: node {}, model {} ({}).\n",
            report.origin.node_id, selected.connection.node_id, selected.model_id, selected.backend
        )
    } else {
        format!(
            "No eligible advertised interpreter for node {} under the current policy and preferences.\n",
            report.origin.node_id
        )
    };
    Ok(
        text + "Read-only routing observation. No allowance, grant or model call was started. Automatic network interpretation is not available yet.\n"
    )
}

#[cfg(test)]
#[path = "interpreter_routing_tests.rs"]
mod tests;
