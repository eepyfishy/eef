//! Explicit local service-panic observation. Never captures panic payloads,
//! backtraces, request content, configuration paths or execution authority.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
};

static PHASE: Mutex<Option<&'static str>> = Mutex::new(None);
const MAX_BYTES: usize = 2048;

pub fn phase(value: &'static str) {
    if let Ok(mut phase) = PHASE.try_lock() {
        *phase = (value.len() <= 64
            && !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .then_some(value);
    }
}

fn snapshot(program: &str, phase: Option<&str>, location: Option<(&str, u32, u32)>) -> Value {
    let location = location.map(|(file, line, column)| {
        let name = file.rsplit(['/', '\\']).next().unwrap_or("");
        let name = if name.len() <= 128
            && !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            name
        } else {
            "unknown"
        };
        json!({"file_basename":name,"line":line,"column":column})
    });
    json!({"schema_version":1,"report_type":"service_panic_observation",
        "program":program,"process_id":std::process::id(),"phase":phase,"location":location,
        "panic_payload_recorded":false,"backtrace_recorded":false,"execution_authorized":false,
        "outcome_confirmed":false})
}

fn open(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context(
            "Cannot create the requested new panic report; existing files are never overwritten",
        )
}

/// Install after package handoff, before service construction. A new file must
/// be explicitly selected by the local launching user. Its open handle is held
/// for this process; recording never replaces or reopens that path on a panic.
pub fn install(path: &Path, program: &'static str) -> Result<()> {
    if !matches!(program, "eef" | "eefn") {
        bail!("Unsupported panic report program")
    }
    let file = Mutex::new(Some(open(path)?));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Keep the first observation only, even if several threads panic.
        // A poisoned/busy recorder is missing evidence, never a clean outcome.
        if let Ok(mut file) = file.try_lock()
            && let Some(mut file) = file.take()
        {
            let phase = PHASE.try_lock().ok().and_then(|phase| *phase);
            let location = info.location().map(|l| (l.file(), l.line(), l.column()));
            if let Ok(bytes) = serde_json::to_vec(&snapshot(program, phase, location))
                && bytes.len() <= MAX_BYTES
            {
                let _ = file.write_all(&bytes);
                let _ = file.flush();
            }
        }
        // Do not suppress normal reporting, catch a panic or continue the service.
        previous(info);
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_omit_private_paths_and_bound_source_names() {
        let value = snapshot(
            "eefn",
            Some("runtime_build"),
            Some((r"C:\PRIVATE_OWNER\secret\signal.rs", 42, 7)),
        );
        assert_eq!(
            value["location"],
            json!({"file_basename":"signal.rs","line":42,"column":7})
        );
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() <= MAX_BYTES);
        assert!(!String::from_utf8(bytes).unwrap().contains("PRIVATE_OWNER"));
        assert_eq!(value["panic_payload_recorded"], false);
        assert_eq!(value["outcome_confirmed"], false);
        assert_eq!(
            snapshot("eef", None, Some((&"x".repeat(129), 1, 1)))["location"]["file_basename"],
            "unknown"
        );
        assert_eq!(snapshot("eef", None, None)["location"], Value::Null);
    }

    #[test]
    fn explicit_report_cannot_overwrite_or_follow_an_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("observation.json");
        std::fs::write(&path, b"owner data").unwrap();
        assert!(open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"owner data");
        let new = dir.path().join("new.json");
        drop(open(&new).unwrap());
        assert!(std::fs::read(new).unwrap().is_empty());
        assert!(open(&dir.path().join("missing-parent/report.json")).is_err());
    }
}
