//! Discovery of this config's actual local API, separate from pending settings.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    schema_version: u32,
    node_id: String,
    address: SocketAddr,
}
fn marker(config: &Path) -> Result<PathBuf> {
    let name = config
        .file_name()
        .and_then(|s| s.to_str())
        .context("configuration file name is required")?;
    Ok(config.with_file_name(format!("{name}.api.json")))
}
pub fn publish(config: &Path, node_id: &str, address: SocketAddr) -> Result<()> {
    let path = marker(config)?;
    let next = path.with_extension("json.next");
    let bytes = serde_json::to_vec(&Endpoint {
        schema_version: 1,
        node_id: node_id.into(),
        address,
    })?;
    std::fs::write(&next, &bytes)?;
    std::fs::rename(&next, &path).or_else(|_| {
        std::fs::copy(&next, &path)?;
        std::fs::remove_file(&next)
    })?;
    Ok(())
}

/// Caller first checks the config's instance lock. Offline commands ignore markers.
pub fn endpoint(
    config: &Path,
    node_id: &str,
    fallback_host: &str,
    fallback_port: u16,
    fallback_enabled: bool,
) -> Result<SocketAddr> {
    let path = marker(config)?;
    let address = match std::fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes)?;
            if bytes.len() > 4096 {
                bail!("local API discovery exceeds bounds")
            }
            let endpoint: Endpoint =
                serde_json::from_slice(&bytes).context("local API discovery is invalid")?;
            if endpoint.schema_version != 1 || endpoint.node_id != node_id {
                bail!("local API discovery does not match this node")
            }
            endpoint.address
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !fallback_enabled {
                bail!("The node's local API is disabled; stop it before editing offline")
            }
            SocketAddr::new(
                fallback_host
                    .parse()
                    .context("local API host must be an IP address")?,
                fallback_port,
            )
        }
        Err(error) => return Err(error.into()),
    };
    if !address.ip().is_loopback() || address.port() == 0 {
        bail!("commands require a loopback local API with a nonzero port")
    }
    Ok(address)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_endpoint_wins_over_pending_port_and_disable_settings() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let actual = "127.0.0.1:12345".parse().unwrap();
        publish(&config, "node", actual).unwrap();
        assert_eq!(
            endpoint(&config, "node", "127.0.0.1", 23456, false).unwrap(),
            actual
        );
        assert!(endpoint(&config, "wrong", "127.0.0.1", 12345, true).is_err());
    }
    #[test]
    fn missing_marker_supports_old_nodes_but_corrupt_or_nonlocal_marker_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        assert_eq!(
            endpoint(&config, "node", "127.0.0.1", 12345, true)
                .unwrap()
                .port(),
            12345
        );
        assert!(endpoint(&config, "node", "127.0.0.1", 12345, false).is_err());
        for raw in ["broken".to_owned(), "x".repeat(4097)] {
            std::fs::write(marker(&config).unwrap(), raw).unwrap();
            assert!(endpoint(&config, "node", "127.0.0.1", 12345, true).is_err());
        }
        publish(&config, "node", "192.0.2.1:12345".parse().unwrap()).unwrap();
        assert!(endpoint(&config, "node", "127.0.0.1", 12345, true).is_err());
    }

    #[tokio::test]
    async fn restart_checks_target_and_runtime_and_only_acknowledges_a_request() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::NodeService::new(dir.path().join("node.json"), "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        core.live.lock().unwrap()["runtime_id"] = serde_json::json!(runtime);
        let make = || RestartRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: Some(runtime.clone()),
        };
        let mut wrong = make();
        wrong.expected_node_id = "wrong".into();
        assert!(core.request_restart(wrong).is_err());
        let mut stale = make();
        stale.expected_runtime_id = None;
        assert!(core.request_restart(stale).is_err());
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                core.restart.notified()
            )
            .await
            .is_err()
        );
        let reply = core.request_restart(make()).unwrap();
        assert_eq!(reply["restart_requested"], true);
        assert_eq!(reply["completed"], false);
        tokio::time::timeout(std::time::Duration::from_secs(2), core.restart.notified())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                core.restart.notified()
            )
            .await
            .is_err()
        );
    }
}
