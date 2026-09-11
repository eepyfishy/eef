use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

const DEFAULT_YAML: &str = include_str!("../../../config/default_identity.yaml");

#[derive(Clone, Debug)]
pub struct Config {
    value: Arc<Value>,
    source: Option<Arc<PathBuf>>,
    edit_lock: Arc<Mutex<()>>,
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let defaults: Value = serde_yml::from_str(DEFAULT_YAML)?;
        let source = path.map(|path| {
            path.canonicalize().unwrap_or_else(|_| {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            })
        });
        let value = if let Some(path) = path.filter(|path| path.is_file()) {
            let loaded: Value = serde_yml::from_slice(&std::fs::read(path)?)
                .with_context(|| format!("parse config {}", path.display()))?;
            if !loaded.is_object() {
                bail!("config must contain a YAML mapping")
            }
            deep_merge(defaults, loaded)
        } else {
            defaults
        };
        Ok(Self {
            value: Arc::new(value),
            source: source.map(Arc::new),
            edit_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn get(&self, path: &str) -> Option<&Value> {
        let mut value = self.value.as_ref();
        for part in path.split('.') {
            value = value.get(part)?;
        }
        Some(value)
    }

    pub fn string(&self, path: &str, default: &str) -> String {
        self.get(path)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .into()
    }

    pub fn u64(&self, path: &str, default: u64) -> u64 {
        self.get(path).and_then(Value::as_u64).unwrap_or(default)
    }

    pub fn bool(&self, path: &str, default: bool) -> bool {
        self.get(path).and_then(Value::as_bool).unwrap_or(default)
    }

    pub fn strings(&self, path: &str) -> Vec<String> {
        self.get(path)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    }

    pub fn object(&self, path: &str) -> Map<String, Value> {
        self.get(path)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    pub fn as_value(&self) -> Value {
        self.value.as_ref().clone()
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref().map(PathBuf::as_path)
    }

    pub fn save_for_restart(&self, value: &Value) -> Result<()> {
        let _guard = self
            .edit_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("configuration lock unavailable"))?;
        self.save_unlocked(value)
    }

    /// Serialize read/modify/save using saved settings, not the startup snapshot.
    pub fn edit_for_restart<T>(
        &self,
        edit: impl FnOnce(&mut Value) -> Result<(T, bool)>,
    ) -> Result<T> {
        let _guard = self
            .edit_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("configuration lock unavailable"))?;
        let path = self
            .source()
            .context("no configuration path was supplied at startup")?;
        if !path.is_file() {
            bail!("saved configuration is missing; refusing to replace it with defaults")
        }
        let mut value = Self::load(Some(path))?.as_value();
        let (result, changed) = edit(&mut value)?;
        if changed {
            self.save_unlocked(&value)?;
        }
        Ok(result)
    }

    fn save_unlocked(&self, value: &Value) -> Result<()> {
        if !value.is_object() {
            bail!("configuration must be a JSON/YAML object")
        }
        crate::discovery::DiscoveryPolicy::parse(value.get("discovery"))?;
        for section in [
            "web", "node", "models", "update", "python", "identity", "jobs",
        ] {
            if value.get(section).is_some_and(|v| !v.is_object()) {
                bail!("{section} settings must be an object")
            }
        }
        for (key, minimum) in [("max_count", 1), ("max_bytes", 1024)] {
            if value
                .pointer(&format!("/jobs/{key}"))
                .is_some_and(|v| !v.as_u64().is_some_and(|n| n >= minimum))
            {
                bail!("jobs.{key} must be a whole number of at least {minimum}")
            }
        }
        for section in ["web", "node"] {
            if let Some(port) = value.pointer(&format!("/{section}/port")) {
                if !port.as_u64().is_some_and(|p| p > 0 && p <= 65535) {
                    bail!("{section} port must be between 1 and 65535")
                }
            }
        }
        if let Some(policy) = value.pointer("/update/policy") {
            crate::update::UpdatePolicy::parse(
                policy.as_str().context("Update preference must be text")?,
            )?;
        }
        let path = self
            .source()
            .context("no configuration path was supplied at startup")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.is_file() {
            std::fs::copy(path, path.with_extension("yaml.bak"))?;
        }
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.yaml");
        let next = path.with_file_name(format!("{filename}.next"));
        std::fs::write(&next, serde_yml::to_string(value)?)?;
        std::fs::rename(&next, path).or_else(|_| {
            std::fs::copy(&next, path)?;
            std::fs::remove_file(&next)
        })?;
        Ok(())
    }
}

fn deep_merge(mut base: Value, overlay: Value) -> Value {
    match (&mut base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                let old = base.remove(&key).unwrap_or(Value::Null);
                base.insert(key, deep_merge(old, value));
            }
            Value::Object(base.clone())
        }
        (_, overlay) => overlay,
    }
}

pub fn embedded_defaults_for_test() -> Value {
    json!({"identity": {"name": "EEF"}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_dotted_access() {
        let config = Config::load(None).unwrap();
        assert_eq!(config.string("identity.name", ""), "EEF");
        assert_eq!(config.string("missing", "fallback"), "fallback");
    }

    #[test]
    fn saved_job_limits_are_validated_without_changing_applied_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eef.yaml");
        let applied = Config::load(Some(&path)).unwrap();
        let mut value = applied.as_value();
        value["jobs"] = json!({"max_count":250,"max_bytes":32*1024*1024});
        applied.save_for_restart(&value).unwrap();
        assert_eq!(applied.u64("jobs.max_count", 0), 500);
        assert_eq!(
            Config::load(Some(&path)).unwrap().u64("jobs.max_count", 0),
            250
        );
        let saved = std::fs::read(&path).unwrap();
        for invalid in [
            json!({"max_count":0}),
            json!({"max_bytes":1023}),
            json!({"max_count":2.5}),
            json!({"max_bytes":"1048576"}),
            Value::Null,
        ] {
            value["jobs"] = invalid;
            assert!(applied.save_for_restart(&value).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), saved);
        }
    }
}
