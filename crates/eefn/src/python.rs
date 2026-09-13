//! Isolated bridge to the CPython runtime shipped in the Windows bundle.
//!
//! Python is an extension boundary, never a hidden implementation dependency:
//! node transport, crypto, routing, updates, and built-in capabilities are Rust.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const PLUGIN_HOST: &str = include_str!("../../../python/eef_plugin_host.py");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PythonPlugin {
    pub path: PathBuf,
    pub capability: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug)]
pub struct PythonRuntime {
    executable: PathBuf,
    timeout: Duration,
}

impl PythonRuntime {
    pub fn discover(explicit: Option<PathBuf>) -> Option<Self> {
        let executable = explicit
            .or_else(|| std::env::var_os("EEF_PYTHON").map(PathBuf::from))
            .or_else(bundled_python)
            .unwrap_or_else(|| {
                PathBuf::from(if cfg!(windows) {
                    "python.exe"
                } else {
                    "python3"
                })
            });
        Some(Self {
            executable,
            timeout: Duration::from_secs(30),
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Probe only the configured interpreter, without importing a device plugin.
    pub async fn check_available(&self) -> Result<()> {
        let mut command = Command::new(&self.executable);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command
            .arg("-I")
            .arg("-c")
            .arg("pass")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Python interpreter could not start")?;
        let status = timeout(Duration::from_secs(5), child.wait())
            .await
            .context("Python interpreter availability check timed out")??;
        if !status.success() {
            bail!("Python interpreter availability check failed")
        }
        Ok(())
    }

    pub async fn inspect(&self, plugin_path: impl AsRef<Path>) -> Result<PythonPlugin> {
        let path = plugin_path.as_ref().to_path_buf();
        let value = self.call(&path, json!({"op": "inspect"})).await?;
        let mut plugin: PythonPlugin =
            serde_json::from_value(value).context("Python plugin returned invalid metadata")?;
        plugin.path = path;
        if plugin.capability.trim().is_empty() {
            bail!("Python plugin capability must not be empty");
        }
        if plugin.actions.is_empty() {
            plugin.actions.push("run".into());
        }
        Ok(plugin)
    }

    pub async fn invoke(
        &self,
        plugin: &PythonPlugin,
        action: &str,
        params: Value,
    ) -> Result<Value> {
        if !plugin
            .actions
            .iter()
            .any(|candidate| candidate == "*" || candidate == action)
        {
            bail!(
                "Python plugin '{}' does not support action '{action}'",
                plugin.capability
            );
        }
        self.call(
            &plugin.path,
            json!({"op": "invoke", "action": action, "params": params}),
        )
        .await
    }

    async fn call(&self, plugin_path: &Path, request: Value) -> Result<Value> {
        if !plugin_path.is_file() {
            bail!("Python plugin '{}' does not exist", plugin_path.display());
        }
        let mut command = Command::new(&self.executable);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command
            .arg("-I")
            .arg("-c")
            .arg(PLUGIN_HOST)
            .arg(plugin_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "could not start bundled Python at '{}'",
                    self.executable.display()
                )
            })?;
        child
            .stdin
            .take()
            .context("Python stdin unavailable")?
            .write_all(&serde_json::to_vec(&request)?)
            .await?;
        let output = timeout(self.timeout, child.wait_with_output())
            .await
            .context("Python plugin timed out")??;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Python plugin failed: {}", stderr.trim());
        }
        let response: Value = serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "Python plugin returned invalid JSON: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })?;
        if response.get("ok").and_then(Value::as_bool) != Some(true) {
            bail!(
                "{}",
                response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Python plugin failed")
            );
        }
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }
}

fn bundled_python() -> Option<PathBuf> {
    let root = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidates = if cfg!(windows) {
        vec![root.join("python/python.exe")]
    } else {
        vec![
            root.join("python/bin/python3"),
            root.join("python/bin/python"),
        ]
    };
    candidates.into_iter().find(|path| path.is_file())
}
