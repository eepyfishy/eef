//! Typed owner configuration only. No approval, recording, playback or listener.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};

const NOTE: &str = "Relay configuration only. Permissions, pairing, discovery grants and workload approval are unchanged. No listener, capture, playback, model call, restart or retry was started. A configured address is not peer reachability or physical audio proof.";
const CAPS: [&str; 3] = ["peer.transfer", "audio.capture", "audio.play"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// Prepare a microphone resource; never enable or record it.
    #[arg(long)]
    #[serde(default)]
    pub microphone: bool,
    /// Prepare speaker receiving on this node; never open a listener.
    #[arg(long, conflicts_with = "send_only")]
    #[serde(default)]
    pub receive: bool,
    /// Clear the receiving route; existing resources/permissions are preserved.
    #[arg(long, conflicts_with = "receive")]
    #[serde(default)]
    pub send_only: bool,
    /// Local numeric IP override; otherwise use the authenticated control interface.
    #[arg(long, requires = "receive")]
    pub address: Option<IpAddr>,
    /// Advanced override; otherwise retain a matching route or choose an OS port.
    #[arg(long, requires = "receive", value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    /// Choose an existing local transport resource if there is more than one.
    #[arg(long)]
    pub transport_resource: Option<String>,
    #[arg(long, requires = "microphone")]
    pub microphone_resource: Option<String>,
    #[arg(long, requires = "receive")]
    pub speaker_resource: Option<String>,
    /// Optional existing adapter index; absent preserves a binding or uses the OS default.
    #[arg(long, requires = "microphone", value_parser = clap::value_parser!(u32).range(..=65535))]
    pub microphone_device: Option<u32>,
}
impl Configuration {
    pub fn validate(&self) -> Result<()> {
        if !(self.microphone || self.receive || self.send_only)
            || self.receive && self.send_only
            || !self.receive
                && (self.address.is_some()
                    || self.port.is_some()
                    || self.speaker_resource.is_some())
            || !self.microphone
                && (self.microphone_device.is_some() || self.microphone_resource.is_some())
            || self.port == Some(0)
            || self.microphone_device.is_some_and(|n| n > 65535)
            || !(self.microphone || self.receive) && self.transport_resource.is_some()
        {
            bail!("Choose microphone, receive or send-only; selectors require their matching role")
        }
        if let Some(ip) = self.address {
            validate_ip(ip)?;
        }
        for id in [
            &self.transport_resource,
            &self.microphone_resource,
            &self.speaker_resource,
        ]
        .into_iter()
        .flatten()
        {
            let local = if let Some((owner, local)) = id.split_once("::") {
                crate::network::validate_remote_node_id(owner)?;
                local
            } else {
                id.as_str()
            };
            if local.is_empty()
                || local.len() > 100
                || !local
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                bail!("Select an existing resource ID, not a path")
            }
        }
        Ok(())
    }
}
#[derive(Debug, clap::Subcommand)]
pub enum RelayAction {
    /// Show configured resources, permission switches and next steps; no device probes.
    Show,
    /// Save resource/receiver setup only; permissions and execution remain separate.
    Configure(Configuration),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelayCommand {
    Show,
    Configure {
        settings: Configuration,
        expected_digest: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: RelayCommand,
}
impl RelayRequest {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported relay setup schema")
        }
        if let RelayCommand::Configure {
            settings,
            expected_digest,
        } = &self.command
        {
            settings.validate()?;
            for selected in [
                &settings.transport_resource,
                &settings.microphone_resource,
                &settings.speaker_resource,
            ]
            .into_iter()
            .flatten()
            {
                local_selector(&self.expected_node_id, selected)?;
            }
            if !crate::request_result::valid_digest(expected_digest) {
                bail!("Exact relay configuration digest required")
            }
        }
        Ok(())
    }
}
fn validate_ip(ip: IpAddr) -> Result<()> {
    crate::peer::plan::validate_address(SocketAddr::new(ip, 1))?;
    if ip == IpAddr::V4(std::net::Ipv4Addr::BROADCAST) {
        bail!("Use a local unicast receiver address")
    }
    Ok(())
}
fn digest(config: &Value) -> Result<String> {
    crate::grants::definition_digest(
        &json!({"metadata":config.get("metadata").cloned().unwrap_or(json!({})),
        "peer_destination_address":config.get("peer_destination_address").cloned().unwrap_or(Value::Null)}),
    )
}
fn receiver(config: &Value) -> Result<Option<SocketAddr>> {
    Ok(serde_json::from_value(
        config
            .get("peer_destination_address")
            .cloned()
            .unwrap_or(Value::Null),
    )?)
}
fn local_selector<'a>(node: &str, id: &'a str) -> Result<&'a str> {
    if let Some((owner, local)) = id.split_once("::") {
        if owner != node {
            bail!("Relay setup cannot select another node's resource")
        }
        Ok(local)
    } else {
        Ok(id)
    }
}
fn ensure_resource(config: &mut Value, cap: &str, selected: Option<&str>) -> Result<String> {
    let selected = selected
        .map(|id| {
            local_selector(
                config["node_id"]
                    .as_str()
                    .context("Node identity missing")?,
                id,
            )
        })
        .transpose()?;
    let metadata: crate::context::NodeMetadata =
        serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
    metadata.validate()?;
    let matching = metadata
        .resources
        .iter()
        .filter(|r| r.capability == cap && selected.is_none_or(|id| id == r.id))
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        bail!("Multiple {cap} resources: choose the existing local resource explicitly")
    }
    if let Some(resource) = matching.first() {
        if !resource.available
            || resource
                .parameters
                .keys()
                .any(|key| cap != "audio.capture" || key != "device")
        {
            bail!("Selected {cap} resource is disabled or has unsupported finite-audio bindings")
        }
        return Ok(resource.id.clone());
    }
    if selected.is_some() {
        bail!("Selected {cap} resource does not exist or has another capability")
    }
    let id = format!("relay-{}", uuid::Uuid::new_v4().simple());
    let root = config
        .as_object_mut()
        .context("Node settings must be an object")?;
    let metadata = root
        .entry("metadata")
        .or_insert(json!({}))
        .as_object_mut()
        .context("Metadata must be an object")?;
    let resources = metadata
        .entry("resources")
        .or_insert(json!([]))
        .as_array_mut()
        .context("Resources must be an array")?;
    resources.push(json!({"id":id,"name":cap,"capability":cap,"available":true}));
    Ok(id)
}
fn apply(config: &mut Value, settings: &Configuration, live: &Value) -> Result<()> {
    settings.validate()?;
    if settings.microphone || settings.receive {
        ensure_resource(
            config,
            "peer.transfer",
            settings.transport_resource.as_deref(),
        )?;
    }
    if settings.microphone {
        let id = ensure_resource(
            config,
            "audio.capture",
            settings.microphone_resource.as_deref(),
        )?;
        if let Some(device) = settings.microphone_device {
            let resource = config["metadata"]["resources"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["id"] == id)
                .unwrap();
            let bindings = resource
                .as_object_mut()
                .unwrap()
                .entry("parameters")
                .or_insert(json!({}));
            bindings
                .as_object_mut()
                .context("Resource bindings must be an object")?
                .insert("device".into(), json!(device));
        }
    }
    if settings.receive {
        ensure_resource(config, "audio.play", settings.speaker_resource.as_deref())?;
        let previous = receiver(config)?;
        let observed = if live["connection"]["state"] == "connected"
            && live["connection"]["transport"] == "pinned_tls13"
        {
            live["control_local_ip"]
                .as_str()
                .and_then(|s| s.parse().ok())
        } else {
            None
        };
        let ip = settings.address.or(observed).or(previous.map(|a| a.ip())).context("No authenticated local interface is available; connect this node first or supply --address")?;
        validate_ip(ip)?;
        let port = settings
            .port
            .or(previous.filter(|a| a.ip() == ip).map(|a| a.port()));
        let address = if let Some(port) = port {
            SocketAddr::new(ip, port)
        } else {
            // Bind only, never listen/accept/connect. The port is a configuration
            // candidate, not a reservation/reachability promise; grant bind can fail.
            let socket = if ip.is_ipv4() {
                tokio::net::TcpSocket::new_v4()?
            } else {
                tokio::net::TcpSocket::new_v6()?
            };
            socket.bind(SocketAddr::new(ip, 0))?;
            socket.local_addr()?
        };
        crate::peer::plan::validate_address(address)?;
        config["peer_destination_address"] = json!(address);
    } else if settings.send_only {
        config["peer_destination_address"] = Value::Null;
    }
    let metadata: crate::context::NodeMetadata =
        serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
    metadata.validate()?;
    if metadata
        .resources
        .iter()
        .filter(|r| r.capability == "peer.transfer" && r.available)
        .count()
        > 8
    {
        bail!("Finite relay supports at most eight available transport resources")
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceView {
    id: String,
    capability: String,
    available: bool,
}
fn resources(node: &str, values: &[crate::context::Resource]) -> Vec<ResourceView> {
    values
        .iter()
        .filter(|r| CAPS.contains(&r.capability.as_str()))
        .map(|r| ResourceView {
            id: crate::context::resource_id(node, &r.id),
            capability: r.capability.clone(),
            available: r.available,
        })
        .collect()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    success: bool,
    node_id: String,
    runtime_id: String,
    command: RelayCommand,
    saved_digest: String,
    changed: bool,
    restart_required: bool,
    saved_resources: Vec<ResourceView>,
    applied_resources: Vec<ResourceView>,
    saved_receive_address: Option<SocketAddr>,
    applied_receive_address: Option<SocketAddr>,
    peer_permission_saved: bool,
    peer_permission_applied: bool,
    microphone_permission_saved: bool,
    microphone_permission_applied: bool,
    output_permission_saved: bool,
    output_permission_applied: bool,
    adapter_capabilities: Vec<String>,
    transport_configured: bool,
    session_advertised: bool,
    execution_authorized: bool,
    restart_requested: bool,
    model_inference_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &RelayRequest) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "node_relay_setup"
        || !report.success
        || report.node_id != request.expected_node_id
        || report.runtime_id != request.expected_runtime_id
        || report.command != request.command
        || !crate::request_result::valid_digest(&report.saved_digest)
        || report.execution_authorized
        || report.restart_requested
        || report.model_inference_requested
        || report.automatically_retried
        || report.note != NOTE
        || report.saved_resources.len() > 128
        || report.applied_resources.len() > 128
        || [report.saved_receive_address, report.applied_receive_address]
            .into_iter()
            .flatten()
            .any(|a| a.port() == 0 || validate_ip(a.ip()).is_err())
        || (report.saved_resources != report.applied_resources
            || report.saved_receive_address != report.applied_receive_address
            || report.peer_permission_saved != report.peer_permission_applied
            || report.microphone_permission_saved != report.microphone_permission_applied
            || report.output_permission_saved != report.output_permission_applied)
            && !report.restart_required
    {
        return false;
    }
    for rows in [&report.saved_resources, &report.applied_resources] {
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let Some(local) = row
                .id
                .strip_prefix(&format!("{}::", request.expected_node_id))
            else {
                return false;
            };
            if local.is_empty()
                || local.len() > 100
                || !local
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || !CAPS.contains(&row.capability.as_str())
                || !seen.insert(&row.id)
            {
                return false;
            }
        }
    }
    if report.adapter_capabilities.len() > 2
        || report
            .adapter_capabilities
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != report.adapter_capabilities.len()
        || report
            .adapter_capabilities
            .iter()
            .any(|c| !["audio.capture", "audio.play"].contains(&c.as_str()))
    {
        return false;
    }
    if let RelayCommand::Configure {
        settings,
        expected_digest,
    } = &request.command
    {
        if report.changed != (report.saved_digest != *expected_digest)
            || report.changed && !report.restart_required
        {
            return false;
        }
        for (wanted, cap, selector) in [
            (
                settings.microphone,
                "audio.capture",
                &settings.microphone_resource,
            ),
            (settings.receive, "audio.play", &settings.speaker_resource),
            (
                settings.microphone || settings.receive,
                "peer.transfer",
                &settings.transport_resource,
            ),
        ] {
            if wanted
                && !report.saved_resources.iter().any(|r| {
                    r.available
                        && r.capability == cap
                        && selector.as_ref().is_none_or(|id| {
                            local_selector(&request.expected_node_id, id).is_ok_and(|local| {
                                r.id == crate::context::resource_id(
                                    &request.expected_node_id,
                                    local,
                                )
                            })
                        })
                })
            {
                return false;
            }
        }
        if settings.send_only && report.saved_receive_address.is_some() {
            return false;
        }
        if settings.receive
            && report.saved_receive_address.is_none_or(|a| {
                validate_ip(a.ip()).is_err()
                    || a.port() == 0
                    || settings.address.is_some_and(|ip| a.ip() != ip)
                    || settings.port.is_some_and(|port| a.port() != port)
            })
        {
            return false;
        }
    } else if report.changed {
        return false;
    }
    true
}
impl crate::NodeService {
    pub fn relay_command(&self, request: RelayRequest, remote: bool) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Relay setup targets another node")
        }
        self.with_permission_runtime(&request.expected_runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                let mut live = self.live.lock().unwrap();
                if live["runtime_id"] != request.expected_runtime_id
                    || config["node_id"] != self.node_id
                {
                    bail!("Relay runtime/identity changed")
                }
                let mut next = config.clone();
                if let RelayCommand::Configure {
                    settings,
                    expected_digest,
                } = &request.command
                {
                    if remote && config.pointer("/management/allow_remote") != Some(&json!(true)) {
                        bail!(
                            "Current local owner approval required for remote relay configuration"
                        )
                    }
                    if digest(config)? != *expected_digest {
                        bail!("Relay configuration changed; inspect it before deciding what to do")
                    }
                    apply(&mut next, settings, &live)?;
                }
                let changed = digest(&next)? != digest(config)?;
                if changed {
                    self.save_config_locked(next.clone())?;
                    live["pending_restart"] = json!(true);
                }
                let metadata: crate::context::NodeMetadata =
                    serde_json::from_value(next.get("metadata").cloned().unwrap_or(json!({})))?;
                metadata.validate()?;
                let saved: crate::NodePolicy =
                    serde_json::from_value(next.get("permissions").cloned().unwrap_or(json!({})))?;
                let applied = engine.applied_permissions();
                let saved_resources = resources(&self.node_id, &metadata.resources);
                let applied_resources = resources(&self.node_id, engine.applied_resources());
                let saved_address = receiver(&next)?;
                let applied_address = engine.peer_receive_address().flatten();
                let binding_changes = json!(
                    metadata
                        .resources
                        .iter()
                        .filter(|r| CAPS.contains(&r.capability.as_str()))
                        .collect::<Vec<_>>()
                ) != json!(
                    engine
                        .applied_resources()
                        .iter()
                        .filter(|r| CAPS.contains(&r.capability.as_str()))
                        .collect::<Vec<_>>()
                );
                let restart_required = live["pending_restart"] == true
                    || binding_changes
                    || saved_resources != applied_resources
                    || saved_address != applied_address
                    || saved.peer_transfers.enabled != applied.peer_transfers.enabled
                    || saved.media.microphone != applied.media.microphone
                    || saved.media.audio_output != applied.media.audio_output;
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_relay_setup".into(),
                    success: true,
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    saved_digest: digest(&next)?,
                    changed,
                    restart_required,
                    saved_resources,
                    applied_resources,
                    saved_receive_address: saved_address,
                    applied_receive_address: applied_address,
                    peer_permission_saved: saved.peer_transfers.enabled,
                    peer_permission_applied: applied.peer_transfers.enabled,
                    microphone_permission_saved: saved.media.microphone,
                    microphone_permission_applied: applied.media.microphone,
                    output_permission_saved: saved.media.audio_output,
                    output_permission_applied: applied.media.audio_output,
                    adapter_capabilities: engine
                        .capabilities()
                        .into_iter()
                        .filter(|c| ["audio.capture", "audio.play"].contains(&c.as_str()))
                        .collect(),
                    transport_configured: live["peer_transport"]["configured"] == true,
                    session_advertised: live["peer_transfer"]["finite_audio"] == true,
                    execution_authorized: false,
                    restart_requested: false,
                    model_inference_requested: false,
                    automatically_retried: false,
                    note: NOTE.into()
                });
                if !confirmed(&report, &request) {
                    bail!("Relay configuration checkpoint failed confirmation; inspect saved state")
                }
                Ok(report)
            })
        })
    }
}
pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    let mut text = format!("Relay setup on node {}\n", report.node_id);
    for (label, saved, applied) in [
        (
            "Peer transfers",
            report.peer_permission_saved,
            report.peer_permission_applied,
        ),
        (
            "Microphone",
            report.microphone_permission_saved,
            report.microphone_permission_applied,
        ),
        (
            "Audio output",
            report.output_permission_saved,
            report.output_permission_applied,
        ),
    ] {
        text.push_str(&format!(
            "{label}: saved {}, applied {}\n",
            if saved { "allowed" } else { "off" },
            if applied { "allowed" } else { "off" }
        ));
    }
    text.push_str(&format!(
        "Receive route: {}\n",
        report
            .saved_receive_address
            .map(|a| a.to_string())
            .unwrap_or("sender-only".into())
    ));
    if report
        .saved_receive_address
        .is_some_and(|a| a.ip().is_loopback())
    {
        text.push_str("This receive address is loopback-only; another PC needs --address with a reachable local interface.\n");
    }
    for resource in &report.saved_resources {
        text.push_str(&format!(
            "{}: {}{}\n",
            resource.capability,
            resource.id,
            if resource.available {
                ""
            } else {
                " (disabled resource)"
            }
        ));
    }
    if !report
        .saved_resources
        .iter()
        .any(|r| r.capability == "peer.transfer")
    {
        text.push_str("First choose a role: eefn relay configure --microphone, --receive, or both. This does not enable permissions.\n");
    }
    if !report.peer_permission_saved {
        text.push_str("Next: eefn permissions allow peer-transfers (separate owner consent).\n");
    }
    if report
        .saved_resources
        .iter()
        .any(|r| r.capability == "audio.capture")
        && !report.microphone_permission_saved
    {
        text.push_str("For recording: eefn permissions allow microphone.\n");
    }
    if report.saved_receive_address.is_some() && !report.output_permission_saved {
        text.push_str("For playback: eefn permissions allow audio-output.\n");
    }
    if report.restart_required {
        text.push_str("Saved changes are not all applied. Explicitly use eefn restart, then relay show. Restart may interrupt work.\n");
    }
    if !report
        .adapter_capabilities
        .iter()
        .any(|c| c == "audio.capture")
    {
        text.push_str("Microphone adapter is not currently available; permission, bundled Python and audio dependencies may need setup.\n");
    }
    if !report
        .adapter_capabilities
        .iter()
        .any(|c| c == "audio.play")
    {
        text.push_str("Playback adapter is not currently available. Current built-in playback uses the Windows default output.\n");
    }
    text.push_str(if report.session_advertised {"Finite audio is advertised on the current control session. This is not a peer/audio test.\n"} else {"Finite audio is not currently advertised; inspect connection, permissions and runtime setup.\n"});
    text.push_str(NOTE);
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
#[path = "relay_setup_tests.rs"]
mod tests;
