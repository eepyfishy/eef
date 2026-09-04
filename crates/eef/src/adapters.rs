use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use eefn::{PythonPlugin, PythonRuntime};
use serde_json::{Value, json};
use tokio::process::Command;

use crate::capability::{CapabilityProvider, CapabilityRegistry};

#[async_trait]
pub trait CapabilityAdapter: Send + Sync {
    fn capability(&self) -> &str;
    fn actions(&self) -> &[&str];
    fn properties(&self) -> Value {
        json!({})
    }
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
    async fn invoke(&self, action: &str, params: Value) -> Result<Value>;
}

#[derive(Clone)]
pub struct AdapterManager {
    registry: CapabilityRegistry,
    adapters: Arc<RwLock<HashMap<String, Arc<dyn CapabilityAdapter>>>>,
    policy: Arc<RwLock<Value>>,
}

impl AdapterManager {
    pub fn new(registry: CapabilityRegistry, policy: Value) -> Self {
        Self {
            registry,
            adapters: Arc::new(RwLock::new(HashMap::new())),
            policy: Arc::new(RwLock::new(policy)),
        }
    }

    pub async fn load(&self, adapter: Arc<dyn CapabilityAdapter>) -> Result<()> {
        adapter.initialize().await?;
        let action = if adapter.actions().len() == 1 {
            adapter.actions()[0]
        } else {
            "*"
        };
        self.registry.register(CapabilityProvider::local(
            adapter.capability(),
            action,
            adapter.properties(),
        ));
        self.adapters
            .write()
            .expect("adapter lock")
            .insert(adapter.capability().into(), adapter);
        Ok(())
    }

    pub async fn invoke(&self, capability: &str, action: &str, params: Value) -> Result<Value> {
        self.check_allowed(capability, action)?;
        self.registry.validate_params(capability, action, &params)?;
        let adapter = self
            .adapters
            .read()
            .expect("adapter lock")
            .get(capability)
            .cloned()
            .with_context(|| format!("no local adapter for '{capability}'"))?;
        adapter.invoke(action, params).await
    }

    pub fn capabilities(&self) -> Vec<String> {
        let mut values = self
            .adapters
            .read()
            .expect("adapter lock")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        values.sort();
        values
    }

    pub async fn shutdown(&self) {
        let adapters = self
            .adapters
            .read()
            .expect("adapter lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for adapter in adapters {
            let _ = adapter.shutdown().await;
        }
    }

    fn check_allowed(&self, capability: &str, action: &str) -> Result<()> {
        let always_sensitive = [
            "system.shell",
            "keyboard",
            "screen_capture",
            "servo",
            "gpio",
            "motor",
            "relay",
            "led",
        ];
        let mutating_file =
            capability == "filesystem" && ["write", "delete", "mkdir"].contains(&action);
        if !always_sensitive.contains(&capability) && !mutating_file {
            return Ok(());
        }
        let policy = self.policy.read().expect("policy lock");
        let entry = policy.get(capability);
        if entry
            .and_then(|entry| entry.get("allow"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            bail!("'{capability}.{action}' denied: requires explicit permission");
        }
        if entry
            .and_then(|entry| entry.get("deny"))
            .and_then(Value::as_array)
            .is_some_and(|denied| denied.iter().any(|item| item.as_str() == Some(action)))
        {
            bail!("'{capability}.{action}' denied by policy");
        }
        Ok(())
    }
}

pub struct FilesystemAdapter {
    roots: Vec<PathBuf>,
}

impl FilesystemAdapter {
    pub fn new(roots: Vec<PathBuf>) -> Result<Self> {
        Ok(Self {
            roots: roots
                .into_iter()
                .map(|path| normalize(&path))
                .collect::<Result<_>>()?,
        })
    }
    fn resolve(&self, raw: &str) -> Result<PathBuf> {
        if raw.is_empty() {
            bail!("filesystem requires 'path'")
        }
        let path = normalize(Path::new(raw))?;
        if !self.roots.is_empty() && !self.roots.iter().any(|root| path.starts_with(root)) {
            bail!("path '{}' outside allowed roots", path.display())
        }
        Ok(path)
    }
}

const FILE_ACTIONS: &[&str] = &["read", "write", "list", "mkdir", "delete"];

#[async_trait]
impl CapabilityAdapter for FilesystemAdapter {
    fn capability(&self) -> &str {
        "filesystem"
    }
    fn actions(&self) -> &[&str] {
        FILE_ACTIONS
    }
    fn properties(&self) -> Value {
        json!({"params": {
            "path": {"type": "string", "required": true},
            "content": {"type": "string", "required": false},
            "recursive": {"type": "boolean", "required": false}
        }})
    }
    async fn invoke(&self, action: &str, params: Value) -> Result<Value> {
        let path = self.resolve(params.get("path").and_then(Value::as_str).unwrap_or(""))?;
        match action {
            "read" => Ok(json!({"path": path, "content": tokio::fs::read_to_string(&path).await?})),
            "write" => {
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
                tokio::fs::create_dir_all(&path).await?;
                Ok(json!({"path": path, "created": true}))
            }
            "delete" => {
                let metadata = tokio::fs::metadata(&path).await?;
                if metadata.is_dir() {
                    tokio::fs::remove_dir(&path).await?
                } else {
                    tokio::fs::remove_file(&path).await?
                };
                Ok(json!({"path": path, "deleted": true}))
            }
            "list" => {
                let mut entries = Vec::new();
                if params.get("recursive").and_then(Value::as_bool) == Some(true) {
                    let mut pending = vec![path.clone()];
                    while let Some(directory) = pending.pop() {
                        let mut reader = tokio::fs::read_dir(directory).await?;
                        while let Some(entry) = reader.next_entry().await? {
                            let is_dir = entry.file_type().await?.is_dir();
                            if is_dir {
                                pending.push(entry.path());
                            }
                            entries.push(json!({"name": entry.file_name().to_string_lossy(), "path": entry.path(), "is_dir": is_dir}));
                        }
                    }
                } else {
                    let mut reader = tokio::fs::read_dir(&path).await?;
                    while let Some(entry) = reader.next_entry().await? {
                        entries.push(json!({"name": entry.file_name().to_string_lossy(), "path": entry.path(), "is_dir": entry.file_type().await?.is_dir()}));
                    }
                }
                entries.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
                Ok(json!({"path": path, "entries": entries}))
            }
            _ => bail!("unknown filesystem action '{action}'"),
        }
    }
}

pub struct LaunchApplicationAdapter {
    commands: BTreeMap<String, Vec<PathBuf>>,
    running: Mutex<HashMap<String, u32>>,
}

impl Default for LaunchApplicationAdapter {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

impl LaunchApplicationAdapter {
    pub fn new(commands: BTreeMap<String, Vec<PathBuf>>) -> Self {
        Self {
            commands,
            running: Mutex::new(HashMap::new()),
        }
    }

    fn resolve(&self, name: &str) -> Result<PathBuf> {
        if let Some(candidates) = self.commands.get(&name.to_lowercase()) {
            for candidate in candidates {
                if candidate.is_absolute() && candidate.is_file() {
                    return Ok(candidate.clone());
                }
                if candidate.components().count() == 1 {
                    if let Some(found) = find_on_path(candidate) {
                        return Ok(found);
                    }
                }
            }
        }
        find_on_path(Path::new(name)).with_context(|| format!("no command found for '{name}'"))
    }
}

const LAUNCH_ACTIONS: &[&str] = &["launch", "terminate", "list"];

#[async_trait]
impl CapabilityAdapter for LaunchApplicationAdapter {
    fn capability(&self) -> &str {
        "launch_application"
    }
    fn actions(&self) -> &[&str] {
        LAUNCH_ACTIONS
    }
    fn properties(&self) -> Value {
        json!({"params": {"name": {"type": "string"}, "args": {"type": "list"}}})
    }
    async fn invoke(&self, action: &str, params: Value) -> Result<Value> {
        if action == "list" {
            return Ok(
                json!({"applications": self.running.lock().expect("app lock").iter().map(|(name,pid)| json!({"name": name, "pid": pid})).collect::<Vec<_>>() }),
            );
        }
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            bail!("{action} requires 'name'")
        }
        match action {
            "launch" => {
                let executable = self.resolve(name)?;
                let mut command = Command::new(executable);
                if let Some(args) = params.get("args").and_then(Value::as_array) {
                    command.args(args.iter().filter_map(Value::as_str));
                }
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(false);
                let child = command.spawn()?;
                let pid = child.id().context("launched process has no pid")?;
                self.running
                    .lock()
                    .expect("app lock")
                    .insert(name.into(), pid);
                Ok(json!({"name": name, "pid": pid}))
            }
            "terminate" => {
                let pid = self.running.lock().expect("app lock").remove(name);
                if let Some(pid) = pid {
                    #[cfg(windows)]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/F"])
                            .output()
                            .await;
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = Command::new("kill").arg(pid.to_string()).output().await;
                    }
                }
                Ok(json!({"name": name, "terminated": pid.is_some()}))
            }
            _ => bail!("unknown launch action '{action}'"),
        }
    }
}

pub struct PythonAdapter {
    runtime: Arc<PythonRuntime>,
    plugin: PythonPlugin,
    action_names: Vec<String>,
}

impl PythonAdapter {
    pub fn new(runtime: Arc<PythonRuntime>, plugin: PythonPlugin) -> Self {
        let action_names = plugin.actions.clone();
        Self {
            runtime,
            plugin,
            action_names,
        }
    }
}

#[async_trait]
impl CapabilityAdapter for PythonAdapter {
    fn capability(&self) -> &str {
        &self.plugin.capability
    }
    fn actions(&self) -> &[&str] {
        // Trait needs borrowed str slices; Python metadata remains exposed as a
        // wildcard provider and validates actions in PythonRuntime::invoke.
        &["*"]
    }
    fn properties(&self) -> Value {
        json!({"params": self.plugin.params, "python": true, "actions": self.action_names})
    }
    async fn invoke(&self, action: &str, params: Value) -> Result<Value> {
        self.runtime.invoke(&self.plugin, action, params).await
    }
}

fn find_on_path(command: &Path) -> Option<PathBuf> {
    if command.components().count() != 1 {
        return command.is_file().then(|| command.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let extensions = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(&path) {
        let direct = directory.join(command);
        if direct.is_file() {
            return Some(direct);
        }
        if command.extension().is_none() {
            for extension in &extensions {
                let candidate = directory.join(format!(
                    "{}{}",
                    command.to_string_lossy(),
                    extension.to_lowercase()
                ));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn normalize(path: &Path) -> Result<PathBuf> {
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
