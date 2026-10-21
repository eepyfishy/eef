//! Regression checks through the actual first-run and owner configuration writers.
use serde_json::json;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.json");
    let mut value = crate::setup::defaults();
    value["node_id"] = json!("node");
    value["name"] = json!("Original node");
    crate::setup::write_json(&path, &value).unwrap();
    (dir, path, value)
}

#[test]
fn atomic_settings_first_run_preserves_preexisting_next_sibling() {
    let (_dir, path, mut value) = fixture();
    let sibling = path.with_extension("json.next");
    let unrelated = b"Existing sibling; not owned by this write";
    std::fs::write(&sibling, unrelated).unwrap();
    value["name"] = json!("New node");
    crate::setup::write_json(&path, &value).unwrap();
    assert_eq!(std::fs::read(&sibling).unwrap(), unrelated);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap(),
        value
    );
}

#[test]
fn atomic_settings_owner_save_preserves_preexisting_next_sibling() {
    let (_dir, path, mut value) = fixture();
    let sibling = path.with_file_name("node.json.next");
    let unrelated = b"Existing sibling; not owned by this write";
    std::fs::write(&sibling, unrelated).unwrap();
    let service = crate::NodeService::new(path.clone(), "node".into());
    value["name"] = json!("New node");
    service.save_configuration(value).unwrap();
    assert_eq!(std::fs::read(sibling).unwrap(), unrelated);
    assert_eq!(service.read_config().unwrap()["name"], "New node");
}

#[cfg(windows)]
#[test]
fn atomic_settings_first_run_refuses_delete_locked_target_without_copy_overwrite() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_dir, path, mut value) = fixture();
    let before = std::fs::read(&path).unwrap();
    // Permit reads/writes but withhold FILE_SHARE_DELETE: rename must fail, while
    // an unsafe copy-overwrite fallback can still replace the old contents.
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    value["name"] = json!("Must not be published");
    assert!(crate::setup::write_json(&path, &value).is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn atomic_settings_owner_save_refuses_delete_locked_target_without_copy_overwrite() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_dir, path, mut value) = fixture();
    let before = std::fs::read(&path).unwrap();
    let service = crate::NodeService::new(path.clone(), "node".into());
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    value["name"] = json!("Must not be published");
    assert!(service.save_configuration(value).is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_ne!(service.live.lock().unwrap()["pending_restart"], true);
}
