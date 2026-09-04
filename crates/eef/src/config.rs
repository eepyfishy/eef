use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

const DEFAULT_YAML: &str = include_str!("../../../config/default_identity.yaml");

#[derive(Clone, Debug)]
pub struct Config {
    value: Arc<Value>,
    source: Option<Arc<PathBuf>>,
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
        if !value.is_object() {
            bail!("configuration must be a JSON/YAML object")
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
}
