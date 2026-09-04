use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::process::Command;

use crate::model_server::ModelServer;
use crate::python::{PythonPlugin, PythonRuntime};
use crate::updater;

pub const CAPABILITIES: &[&str] = &[
    "system.ping",
    "filesystem",
    "launch_application",
    "node.update",
];

const MODEL_CAPABILITIES: &[&str] = &["llm.infer", "vlm.analyze"];

pub struct NodeEngine {
    pub allow_write: bool,
    allowed_roots: Vec<PathBuf>,
    pub model_server: Option<Arc<ModelServer>>,
    python: Option<Arc<PythonRuntime>>,
    plugins: HashMap<String, PythonPlugin>,
    install_dir: PathBuf,
    update_manifest: Option<String>,
    current_version: String,
    http: reqwest::Client,
    ollama_url: String,
    ollama_models: HashMap<String, String>,
}

impl NodeEngine {
    pub fn new(
        allow_write: bool,
        allowed_roots: Vec<PathBuf>,
        install_dir: PathBuf,
    ) -> Result<Self> {
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|root| normalize_path(&root))
            .collect::<Result<Vec<_>>>()?;
        let current_version = std::fs::read_to_string(install_dir.join("current.txt"))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| crate::VERSION.into());
        Ok(Self {
            allow_write,
            allowed_roots,
            model_server: None,
            python: None,
            plugins: HashMap::new(),
            install_dir,
            update_manifest: None,
            current_version,
            http: reqwest::Client::new(),
            ollama_url: String::new(),
            ollama_models: HashMap::new(),
        })
    }

    pub fn with_model_server(mut self, server: Arc<ModelServer>) -> Self {
        self.model_server = Some(server);
        self
    }

    pub fn with_update_manifest(mut self, manifest: Option<String>) -> Self {
        self.update_manifest = manifest;
        self
    }

    pub fn with_ollama(
        mut self,
        url: String,
        models: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.ollama_url = url;
        self.ollama_models = models.into_iter().collect();
        self
    }

    pub async fn add_python_plugin(
        &mut self,
        runtime: Arc<PythonRuntime>,
        path: PathBuf,
    ) -> Result<()> {
        let plugin = runtime.inspect(path).await?;
        if self.plugins.contains_key(&plugin.capability)
            || CAPABILITIES.contains(&plugin.capability.as_str())
            || MODEL_CAPABILITIES.contains(&plugin.capability.as_str())
        {
            bail!("duplicate capability '{}'", plugin.capability);
        }
        self.plugins.insert(plugin.capability.clone(), plugin);
        self.python = Some(runtime);
        Ok(())
    }

    pub fn capabilities(&self) -> Vec<String> {
        let mut values = CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        values.extend(self.plugins.keys().cloned());
        let has_text = self.ollama_models.values().any(|value| value == "text")
            || self
                .model_server
                .as_ref()
                .is_some_and(|server| server.slots.iter().any(|slot| !slot.is_vlm()));
        let has_vlm = self.ollama_models.values().any(|value| value == "vlm")
            || self
                .model_server
                .as_ref()
                .is_some_and(|server| server.slots.iter().any(|slot| slot.is_vlm()));
        if has_text {
            values.push("llm.infer".into());
        }
        if has_vlm {
            values.push("vlm.analyze".into());
        }
        values.sort();
        values
    }

    pub fn python_plugins(&self) -> Vec<&PythonPlugin> {
        self.plugins.values().collect()
    }

    pub async fn execute(&self, capability: &str, action: &str, params: Value) -> Result<Value> {
        match capability {
            "system.ping" => Ok(json!({
                "pong": true,
                "time": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64(),
                "runtime": "rust",
                "version": crate::VERSION,
            })),
            "filesystem" => self.filesystem(action, params).await,
            "launch_application" => self.launch(action, params).await,
            "llm.infer" | "vlm.analyze" => self.infer(capability, params).await,
            "node.update" => self.node_update(action, params).await,
            other => {
                let plugin = self
                    .plugins
                    .get(other)
                    .with_context(|| format!("unknown capability '{other}'"))?;
                let runtime = self
                    .python
                    .as_ref()
                    .context("Python runtime is unavailable")?;
                runtime.invoke(plugin, action, params).await
            }
        }
    }

    async fn filesystem(&self, action: &str, params: Value) -> Result<Value> {
        let raw = params.get("path").and_then(Value::as_str).unwrap_or("");
        if raw.is_empty() {
            bail!("filesystem requires 'path'")
        }
        let path = self.resolve(raw)?;
        match action {
            "read" => Ok(json!({"path": path, "content": tokio::fs::read_to_string(&path).await?})),
            "list" => {
                let mut directory = tokio::fs::read_dir(&path).await?;
                let mut entries = Vec::new();
                while let Some(entry) = directory.next_entry().await? {
                    entries.push(entry.path().to_string_lossy().into_owned());
                }
                entries.sort();
                Ok(json!({"path": path, "entries": entries}))
            }
            "write" => {
                if !self.allow_write {
                    bail!("filesystem.write is not enabled on this node")
                }
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?
                }
                tokio::fs::write(
                    &path,
                    params.get("content").and_then(Value::as_str).unwrap_or(""),
                )
                .await?;
                Ok(json!({"path": path, "written": true}))
            }
            "mkdir" => {
                if !self.allow_write {
                    bail!("filesystem.mkdir is not enabled on this node")
                }
                tokio::fs::create_dir_all(&path).await?;
                Ok(json!({"path": path, "created": true}))
            }
            _ => bail!("unknown filesystem action '{action}'"),
        }
    }

    fn resolve(&self, raw: &str) -> Result<PathBuf> {
        let path = normalize_path(Path::new(raw))?;
        if !self.allowed_roots.is_empty()
            && !self.allowed_roots.iter().any(|root| path.starts_with(root))
        {
            bail!("path '{}' outside allowed roots", path.display());
        }
        Ok(path)
    }

    async fn launch(&self, action: &str, params: Value) -> Result<Value> {
        if action != "launch" {
            bail!("unknown launch action '{action}'")
        }
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            bail!("launch requires 'name'")
        }
        let mut command = Command::new(name);
        if let Some(args) = params.get("args").and_then(Value::as_array) {
            command.args(args.iter().filter_map(Value::as_str));
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        let child = command
            .spawn()
            .with_context(|| format!("could not launch '{name}'"))?;
        Ok(json!({"launched": name, "pid": child.id()}))
    }

    async fn infer(&self, capability: &str, params: Value) -> Result<Value> {
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .context("model is required and must be selected in the node configuration")?;
        if let Some(server) = &self.model_server {
            if server.slot(Some(model)).is_some() {
                return self.llamacpp(server, &params).await;
            }
        }
        let modality = self
            .ollama_models
            .get(model)
            .with_context(|| format!("model '{model}' is not selected on this node"))?;
        let expected = if capability == "vlm.analyze" {
            "vlm"
        } else {
            "text"
        };
        if modality != expected {
            bail!("model '{model}' is selected as '{modality}', not '{expected}'")
        }
        self.ollama(&params).await
    }

    async fn ollama(&self, params: &Value) -> Result<Value> {
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .context("model is required")?;
        let mut messages = params.get("messages").cloned().unwrap_or_else(|| json!([
            {"role": "user", "content": params.get("prompt").and_then(Value::as_str).unwrap_or("")}
        ]));
        if let (Some(images), Some(last)) = (
            params.get("images"),
            messages.as_array_mut().and_then(|items| items.last_mut()),
        ) {
            if let Some(last) = last.as_object_mut() {
                last.insert("images".into(), images.clone());
            }
        }
        let response = self
            .http
            .post(format!("{}/api/chat", self.ollama_url))
            .timeout(Duration::from_secs(
                params.get("timeout").and_then(Value::as_u64).unwrap_or(60),
            ))
            .json(&json!({
                "model": model, "messages": messages, "stream": false, "keep_alive": -1,
                "options": {
                    "temperature": params.get("temperature").and_then(Value::as_f64).unwrap_or(0.7),
                    "num_predict": params.get("max_tokens").and_then(Value::as_u64).unwrap_or(1024),
                }
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(
            json!({"content": response.pointer("/message/content").and_then(Value::as_str).unwrap_or("")}),
        )
    }

    async fn llamacpp(&self, server: &ModelServer, params: &Value) -> Result<Value> {
        let model = params.get("model").and_then(Value::as_str);
        let base = server
            .base_url(model)
            .context("no llama.cpp model configured")?;
        let images = params.get("images").and_then(Value::as_array);
        let raw_messages = params.get("messages").and_then(Value::as_array).cloned().unwrap_or_else(|| vec![json!({
            "role": "user", "content": params.get("prompt").and_then(Value::as_str).unwrap_or("")
        })]);
        let messages =
            raw_messages
                .into_iter()
                .map(|mut message| {
                    if message.get("role").and_then(Value::as_str) == Some("user") {
                        if let Some(images) = images {
                            let text = message.get("content").and_then(Value::as_str).unwrap_or("");
                            let mut parts = vec![json!({"type": "text", "text": text})];
                            parts.extend(images.iter().filter_map(Value::as_str).map(|image| json!({
                        "type": "image_url", "image_url": {"url": ensure_data_url(image)}
                    })));
                            message["content"] = Value::Array(parts);
                        }
                    }
                    message
                })
                .collect::<Vec<_>>();
        let response = self
            .http
            .post(format!("{base}/v1/chat/completions"))
            .timeout(Duration::from_secs(
                params.get("timeout").and_then(Value::as_u64).unwrap_or(60),
            ))
            .json(&json!({
                "messages": messages,
                "temperature": params.get("temperature").and_then(Value::as_f64).unwrap_or(0.7),
                "max_tokens": params.get("max_tokens").and_then(Value::as_u64).unwrap_or(1024),
                "stream": false,
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let content = response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .context("llama.cpp returned an unexpected response")?;
        Ok(json!({"content": content}))
    }

    async fn node_update(&self, action: &str, params: Value) -> Result<Value> {
        let manifest = params
            .get("manifest_url")
            .and_then(Value::as_str)
            .or(self.update_manifest.as_deref())
            .context("no manifest_url configured")?;
        match action {
            "check" => Ok(serde_json::to_value(
                updater::check(manifest, &self.current_version, Duration::from_secs(30)).await?,
            )?),
            "apply" => Ok(serde_json::to_value(
                updater::apply(manifest, &self.install_dir, Duration::from_secs(120)).await?,
            )?),
            _ => bail!("unknown node.update action '{action}'"),
        }
    }
}

fn ensure_data_url(image: &str) -> String {
    if image.starts_with("data:image")
        || image.starts_with("http://")
        || image.starts_with("https://")
        || image.starts_with("file://")
    {
        image.into()
    } else {
        format!("data:image/jpeg;base64,{image}")
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str())
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("path escapes filesystem root")
                }
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn filesystem_is_permission_gated_and_rooted() {
        let dir = tempfile::tempdir().unwrap();
        let denied = NodeEngine::new(false, vec![dir.path().into()], dir.path().into()).unwrap();
        assert!(
            denied
                .execute(
                    "filesystem",
                    "write",
                    json!({"path": dir.path().join("x"), "content": "a"})
                )
                .await
                .is_err()
        );
        let allowed = NodeEngine::new(true, vec![dir.path().into()], dir.path().into()).unwrap();
        allowed
            .execute(
                "filesystem",
                "write",
                json!({"path": dir.path().join("x"), "content": "a"}),
            )
            .await
            .unwrap();
        let read = allowed
            .execute("filesystem", "read", json!({"path": dir.path().join("x")}))
            .await
            .unwrap();
        assert_eq!(read["content"], "a");
        assert!(
            allowed
                .execute(
                    "filesystem",
                    "read",
                    json!({"path": dir.path().join("../outside")})
                )
                .await
                .is_err()
        );
        assert!(!allowed.capabilities().contains(&"llm.infer".to_owned()));
        assert!(!allowed.capabilities().contains(&"vlm.analyze".to_owned()));
    }
}
