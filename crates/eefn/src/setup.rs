//! Per-user first-run identity and local pairing. No unauthenticated network pairing.
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn defaults() -> Value {
    serde_json::from_str(include_str!("../../../config/node.example.json")).expect("node defaults")
}

pub fn hostname() -> String {
    sysinfo::System::host_name()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "This PC".into())
}

/// Enumerate hardware metadata without opening a camera or recording audio.
pub async fn hardware() -> Value {
    let mut value = crate::system_specs();
    #[cfg(windows)]
    {
        let script = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $gpu=@(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name); $devices=@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.Status -eq 'OK' -and $_.PNPClass -in @('Camera','Image','AudioEndpoint','Monitor')} | Select-Object Name,PNPClass); @{gpu=$gpu;devices=$devices} | ConvertTo-Json -Compress -Depth 4";
        let mut command = tokio::process::Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .kill_on_drop(true)
            .creation_flags(0x0800_0000);
        if let Ok(Ok(output)) =
            tokio::time::timeout(std::time::Duration::from_secs(8), command.output()).await
        {
            if let Ok(extra) = serde_json::from_slice::<Value>(&output.stdout) {
                value["gpu_model"] = json!(
                    extra["gpu"]
                        .as_array()
                        .map(|list| list
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "))
                        .unwrap_or_default()
                );
                value["devices"] = extra["devices"].clone();
                value["inventory_available"] = json!(true);
            }
        }
    }
    value
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let next = path.with_extension("json.next");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&next)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, path).or_else(|_| {
        std::fs::copy(&next, path)?;
        std::fs::remove_file(&next)
    })?;
    Ok(())
}

pub fn initialize_identity(path: &Path) -> Result<Value> {
    let mut value = if path.is_file() {
        serde_json::from_slice(&std::fs::read(path)?)?
    } else {
        defaults()
    };
    if !value.is_object() {
        bail!("Device settings must be an object")
    }
    let before = value.clone();
    let id = value["node_id"].as_str().unwrap_or("");
    if id.trim().is_empty() || id == "node-example" {
        value["node_id"] = json!(format!("device-{}", uuid::Uuid::new_v4()));
    }
    let name = value["name"].as_str().unwrap_or("");
    if name.trim().is_empty() || name == "Example node" {
        value["name"] = json!(hostname());
    }
    if !path.is_file() || value != before {
        write_json(path, &value)?;
    }
    Ok(value)
}

pub fn discovery_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("EEF_DISCOVERY_DIR") {
        return Some(PathBuf::from(path).join("local-eef.json"));
    }
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(|path| {
            PathBuf::from(path)
                .join("EEF Connection")
                .join("local-eef.json")
        })
}

pub fn publish_local(port: u16, dashboard_port: u16, name: &str, psk: &str) -> Result<()> {
    if let Some(path) = discovery_path() {
        write_json(
            &path,
            &json!({"address":format!("127.0.0.1:{port}"),"dashboard_port":dashboard_port,"name":name,"psk":psk}),
        )?;
    }
    Ok(())
}

pub fn local_connection() -> Option<Value> {
    let value: Value = serde_json::from_slice(&std::fs::read(discovery_path()?).ok()?).ok()?;
    let address: std::net::SocketAddr = value["address"].as_str()?.parse().ok()?;
    if !address.ip().is_loopback() || value["psk"].as_str()?.len() < 12 {
        return None;
    }
    Some(value)
}

pub fn pair_local(path: &Path) -> Result<bool> {
    let mut config: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    if config["auto_local"].as_bool() == Some(false) {
        return Ok(false);
    }
    if config["endpoints"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
        && config["local_pairing"].as_bool() != Some(true)
    {
        return Ok(false);
    }
    let Some(local) = local_connection() else {
        return Ok(false);
    };
    let before = config.clone();
    config["endpoints"] = json!([{"address":local["address"],"priority":100}]);
    config["psk"] = local["psk"].clone();
    config["coordinator_name"] = local["name"].clone();
    config["local_pairing"] = json!(true);
    if config != before {
        write_json(path, &config)?;
        return Ok(true);
    }
    Ok(false)
}

/// Reject cross-site browser requests and DNS rebinding against the local controls.
pub async fn local_ui_guard(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let authority = format!("http://{host}");
    let local = reqwest::Url::parse(&authority)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    let foreign = request.headers().get(header::ORIGIN).is_some_and(|v| {
        v.to_str()
            .ok()
            .and_then(|s| reqwest::Url::parse(s).ok())
            .is_none_or(|u| u.scheme() != "http" || u.authority() != host)
    });
    let cross_site = request
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site");
    if !local || foreign || cross_site {
        return (
            StatusCode::FORBIDDEN,
            "Open this dashboard on this PC using its app shortcut.",
        )
            .into_response();
    }
    next.run(request).await
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        bail!("Choose a device name between 1 and 100 characters")
    }
    Ok(())
}

pub fn instance_lock(config: &Path) -> Result<Option<std::fs::File>> {
    use fs2::FileExt;
    let path = config.with_extension("instance.lock");
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn open_dashboard(host: &str, port: u16) {
    let host = if host == "0.0.0.0" { "127.0.0.1" } else { host };
    let url = format!("http://{host}:{port}/");
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = url;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_instance_is_rejected_until_lock_drops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let first = instance_lock(&path).unwrap().unwrap();
        assert!(instance_lock(&path).unwrap().is_none());
        drop(first);
        assert!(instance_lock(&path).unwrap().is_some());
    }
    #[test]
    fn identity_survives_rename_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut first = initialize_identity(&path).unwrap();
        let id = first["node_id"].clone();
        first["name"] = json!("Bedroom");
        write_json(&path, &first).unwrap();
        let next = initialize_identity(&path).unwrap();
        assert_eq!(next["node_id"], id);
        assert_eq!(next["name"], "Bedroom");
        let other = initialize_identity(&dir.path().join("other.json")).unwrap();
        assert_ne!(other["node_id"], id);
    }
    #[test]
    fn sample_identity_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_json(&path,&json!({"node_id":"node-example","name":"Example node","permissions":{"media":{"camera":false}}})).unwrap();
        let value = initialize_identity(&path).unwrap();
        assert_ne!(value["node_id"], "node-example");
        assert_eq!(value["permissions"]["media"]["camera"], false);
    }
}
