//! Local first creation and explicit installer secret preparation, never trust migration.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;

const DEFAULTS: &str = include_str!("../config/default_identity.yaml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Initialization {
    Created,
    Existing,
}

fn existing(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => bail!("Coordinator settings must be a regular file; nothing was replaced"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn prepare_secret(value: &mut Value) -> Result<bool> {
    let current = value
        .pointer("/node/psk")
        .and_then(Value::as_str)
        .unwrap_or("");
    if current.len() >= 12
        && !current.contains("CHANGE_ME")
        && !current.eq_ignore_ascii_case("change-me-eef")
    {
        return Ok(false);
    }
    let object = value
        .as_object_mut()
        .context("EEF settings must be a mapping")?;
    let node = object.entry("node").or_insert_with(|| json!({}));
    node.as_object_mut()
        .context("Node settings must be a mapping")?
        .insert(
            "psk".into(),
            Value::String(hex::encode(rand::random::<[u8; 32]>())),
        );
    Ok(true)
}

/// Ordinary app startup and MSI may create missing mutable settings. Existing
/// bytes, including invalid settings, are never repaired or rotated implicitly.
pub fn initialize(path: &Path) -> Result<Initialization> {
    if existing(path)? {
        return Ok(Initialization::Existing);
    }
    let mut value: Value =
        serde_yml::from_str(DEFAULTS).context("Bundled coordinator template is invalid")?;
    prepare_secret(&mut value)?;
    let bytes = serde_yml::to_string(&value)?.into_bytes();
    match crate::atomic_file::create(path, &bytes) {
        Ok(()) => Ok(Initialization::Created),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && existing(path)? => {
            Ok(Initialization::Existing)
        }
        Err(error) => Err(error.into()),
    }
}

/// Existing EXE installation is an explicit setup operation, retaining its
/// placeholder-secret preparation behavior. Ordinary startup does not call this.
pub fn ensure_secret_file(path: &Path) -> Result<()> {
    if !existing(path)? {
        bail!("Coordinator template is missing; no existing settings were replaced");
    }
    let mut value: Value = serde_yml::from_slice(&std::fs::read(path)?).map_err(|_| {
        anyhow::anyhow!("Coordinator settings are invalid; existing bytes were preserved")
    })?;
    if prepare_secret(&mut value)? {
        crate::atomic_file::write(path, serde_yml::to_string(&value)?.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_first_initializers_retain_one_complete_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.yaml");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let path = path.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                initialize(&path).unwrap()
            }));
        }
        barrier.wait();
        let states: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            states
                .iter()
                .filter(|s| **s == Initialization::Created)
                .count(),
            1
        );
        let value: Value = serde_yml::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["node"]["psk"].as_str().unwrap().len(), 64);
    }
    #[test]
    fn first_creation_persists_distinct_secrets_and_never_rewrites_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("one/config.yaml");
        let two = dir.path().join("two/config.yaml");
        assert_eq!(initialize(&one).unwrap(), Initialization::Created);
        assert_eq!(initialize(&two).unwrap(), Initialization::Created);
        let before = std::fs::read(&one).unwrap();
        let value: Value = serde_yml::from_slice(&before).unwrap();
        let other: Value = serde_yml::from_slice(&std::fs::read(two).unwrap()).unwrap();
        assert_eq!(value["node"]["psk"].as_str().unwrap().len(), 64);
        assert_ne!(value["node"]["psk"], other["node"]["psk"]);
        assert_eq!(initialize(&one).unwrap(), Initialization::Existing);
        assert_eq!(std::fs::read(one).unwrap(), before);
    }
    #[test]
    fn existing_invalid_or_custom_configuration_is_not_repaired_by_startup() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("invalid", b"[invalid YAML".as_slice()),
            (
                "custom",
                b"# owner\nnode:\n  psk: change-me-eef\n".as_slice(),
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(initialize(&path).unwrap(), Initialization::Existing);
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert!(initialize(dir.path()).is_err());
    }
    #[test]
    fn explicit_installer_preparation_preserves_valid_owner_formatting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let bytes = b"# Owner formatting\nnode:\n  psk: valid-owner-secret\n";
        std::fs::write(&path, bytes).unwrap();
        ensure_secret_file(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, b"node:\n  psk: change-me-eef\n").unwrap();
        ensure_secret_file(&path).unwrap();
        let value: Value = serde_yml::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["node"]["psk"].as_str().unwrap().len(), 64);
    }
}
