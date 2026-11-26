//! Read-only setup checks for service startup and package mutations.
use anyhow::{Context, Result, bail, ensure};
use std::{fs::File, path::Path};

pub const JOURNAL_NAME: &str = ".setup-transaction";

/// Point-in-time setup evidence, not application health or authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupState {
    NoPendingSetup,
    Uncommitted,
    CommittedCleanupPending,
}

/// Inspect only. Recovery, truncation and cleanup belong to the installer.
pub fn startup_state(installation: &Path, role: &str) -> Result<StartupState> {
    use crate::setup_journal_format::{file_id, parse, regular};
    ensure!(matches!(role, "eef" | "eefn"), "Unknown setup role");
    let path = installation.join(JOURNAL_NAME);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StartupState::NoPendingSetup);
        }
        Err(error) => return Err(error).context("Cannot inspect the previous setup state"),
        Ok(_) => (),
    }
    let parsed = parse(&path)?;
    parsed.header.check_base(installation)?;
    ensure!(
        parsed.header.role == role,
        "Setup journal belongs to another role"
    );
    for stage in &parsed.stages {
        parsed.header.check_stage(stage)?;
    }
    for change in &parsed.changes {
        parsed.header.check_change(change, &parsed.stages)?;
    }
    regular(&path)?;
    let current = File::open(&path)?;
    ensure!(
        file_id(&current)? == parsed.identity && current.metadata()?.len() == parsed.length,
        "Setup journal changed during inspection"
    );
    // A partial cleanup append does not undo an already validated commit.
    // Leave that tail untouched for installer recovery.
    Ok(if parsed.committed {
        StartupState::CommittedCleanupPending
    } else {
        StartupState::Uncommitted
    })
}

pub fn require_startup_ready(installation: &Path, role: &str) -> Result<()> {
    match startup_state(installation, role).context(
        "Setup state could not be verified. Run the matching installer in this folder before starting the app",
    )? {
        StartupState::NoPendingSetup => Ok(()),
        StartupState::Uncommitted => bail!(
            "Setup did not finish. Run the matching installer in this folder before starting the app"
        ),
        StartupState::CommittedCleanupPending => {
            eprintln!("Setup completed, but backup cleanup is pending. Keep the setup journal and backups; run the matching installer to finish cleanup.");
            Ok(())
        }
    }
}

pub fn require_complete(installation: &Path) -> Result<()> {
    match std::fs::symlink_metadata(installation.join(JOURNAL_NAME)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("Cannot inspect the previous setup state"),
        Ok(_) => bail!(
            "An interrupted or unfinished setup needs recovery. Run the matching installer in this folder before updating or repairing it"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_journal_format::{Header, Record, encode};
    use sha2::{Digest, Sha256};

    fn header(root: &Path) -> serde_json::Value {
        serde_json::to_value(Record::Header {
            header: Header {
                version: 1,
                transaction_id: "0123456789abcdef0123456789abcdef".into(),
                root: root.canonicalize().unwrap(),
                role: "eefn".into(),
                external: vec![],
            },
        })
        .unwrap()
    }

    // Frame independent JSON so invalid schema/ordering still has a valid checksum.
    fn frame(value: &serde_json::Value) -> Vec<u8> {
        let body = serde_json::to_vec(value).unwrap();
        let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&Sha256::digest(body));
        bytes
    }

    fn inspect(root: &Path, records: &[serde_json::Value], tail: &[u8]) -> Result<StartupState> {
        let mut bytes = records.iter().flat_map(frame).collect::<Vec<_>>();
        bytes.extend_from_slice(tail);
        let path = root.join(JOURNAL_NAME);
        std::fs::write(&path, &bytes).unwrap();
        let result = startup_state(root, "eefn");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "Inspection must never recover or truncate"
        );
        result
    }

    #[test]
    fn startup_distinguishes_uncommitted_from_committed_cleanup_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            startup_state(dir.path(), "eefn").unwrap(),
            StartupState::NoPendingSetup
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        let h = header(dir.path());
        for tail in [vec![], vec![5, 0], vec![20, 0, 0, 0, b'{']] {
            assert_eq!(
                inspect(dir.path(), &[h.clone()], &tail).unwrap(),
                StartupState::Uncommitted
            );
            assert!(require_startup_ready(dir.path(), "eefn").is_err());
            assert_eq!(
                inspect(
                    dir.path(),
                    &[h.clone(), serde_json::json!({"kind":"committed"})],
                    &tail
                )
                .unwrap(),
                StartupState::CommittedCleanupPending
            );
            require_startup_ready(dir.path(), "eefn").unwrap();
            assert!(
                require_complete(dir.path()).is_err(),
                "Pending cleanup still blocks another package mutation"
            );
        }
    }

    #[test]
    fn startup_denies_unrecognized_corrupt_or_misbound_journals() {
        let dir = tempfile::tempdir().unwrap();
        let h = header(dir.path());
        let committed = serde_json::json!({"kind":"committed"});
        let mut future = h.clone();
        future["header"]["version"] = 2.into();
        let mut other = h.clone();
        other["header"]["role"] = "eef".into();
        let mut extra = h.clone();
        extra["unexpected"] = true.into();
        let mut wrong_root = h.clone();
        wrong_root["header"]["root"] = serde_json::to_value(dir.path().parent().unwrap()).unwrap();
        let mut bad_id = h.clone();
        bad_id["header"]["transaction_id"] = "invalid".into();
        for records in [
            vec![],
            vec![future],
            vec![other],
            vec![extra],
            vec![wrong_root],
            vec![bad_id],
            vec![h.clone(), h.clone()],
            vec![committed.clone()],
            vec![h.clone(), serde_json::json!({"kind":"future"})],
            vec![h.clone(), committed.clone(), committed],
            vec![h.clone(), serde_json::json!({"kind":"applied","index":0})],
        ] {
            assert!(
                inspect(dir.path(), &records, &[]).is_err(),
                "Accepted {records:?}"
            );
        }
        let path = dir.path().join(JOURNAL_NAME);
        let mut corrupt = frame(&h);
        *corrupt.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &corrupt).unwrap();
        assert!(require_startup_ready(dir.path(), "eefn").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
        for length in [0u32, 1, 256 * 1024 + 1] {
            assert!(inspect(dir.path(), &[h.clone()], &length.to_le_bytes()).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(startup_state(dir.path(), "eefn").is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn startup_requires_applied_changes_before_commit_and_valid_path_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let h = header(&root);
        let identity =
            serde_json::json!({"volume":1,"identifier":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]});
        let stage = serde_json::json!({"kind":"staging","index":0,"stage":{"path":root.join(".eef-setup-stage-test"),"identity":identity}});
        let prepared = serde_json::json!({"kind":"prepared","index":0,"change":{
            "path":root.join("app.exe"),"original":null,"after":vec![0u8;32],"applied":false,
            "after_identity":identity,"reverted":false,"cleaned":false,"stage":0
        }});
        let applied = serde_json::json!({"kind":"applied","index":0});
        let commit = serde_json::json!({"kind":"committed"});
        assert!(
            inspect(
                &root,
                &[h.clone(), stage.clone(), prepared.clone(), commit.clone()],
                &[]
            )
            .is_err()
        );
        assert_eq!(
            inspect(
                &root,
                &[
                    h.clone(),
                    stage.clone(),
                    prepared.clone(),
                    applied.clone(),
                    commit.clone()
                ],
                &[]
            )
            .unwrap(),
            StartupState::CommittedCleanupPending
        );
        assert!(
            inspect(
                &root,
                &[
                    h.clone(),
                    stage.clone(),
                    prepared.clone(),
                    applied,
                    serde_json::json!({"kind":"undone","index":0}),
                    commit
                ],
                &[]
            )
            .is_err()
        );
        let mut outside = stage.clone();
        outside["stage"]["path"] =
            serde_json::to_value(root.parent().unwrap().join(".eef-setup-stage-outside")).unwrap();
        assert!(inspect(&root, &[h.clone(), outside], &[]).is_err());
        let mut redirected = prepared;
        redirected["change"]["path"] =
            serde_json::to_value(root.parent().unwrap().join("outside.exe")).unwrap();
        assert!(inspect(&root, &[h, stage, redirected], &[]).is_err());
    }

    #[test]
    fn v1_commit_wire_bytes_are_unchanged() {
        assert_eq!(
            encode(&Record::Committed).unwrap(),
            frame(&serde_json::json!({"kind":"committed"}))
        );
    }

    #[test]
    #[ignore = "requires explicitly supplied immutable previous-writer fixture manifest"]
    fn previous_writer_journals_remain_readable_without_changes() {
        let manifest =
            std::env::var_os("EEF_SETUP_COMPAT_FIXTURE").expect("Supply fixture.json path");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        for case in manifest["cases"].as_array().unwrap() {
            let root = Path::new(case["root"].as_str().unwrap());
            let bytes = std::fs::read(root.join(JOURNAL_NAME)).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                case["journal_sha256"].as_str().unwrap()
            );
            let expected = if case["committed"] == true {
                StartupState::CommittedCleanupPending
            } else {
                StartupState::Uncommitted
            };
            assert_eq!(
                startup_state(root, case["role"].as_str().unwrap()).unwrap(),
                expected
            );
            assert_eq!(std::fs::read(root.join(JOURNAL_NAME)).unwrap(), bytes);
            for (relative, hash) in case["files"].as_object().unwrap() {
                assert_eq!(
                    format!(
                        "{:x}",
                        Sha256::digest(std::fs::read(root.join(relative)).unwrap())
                    ),
                    hash.as_str().unwrap()
                );
            }
        }
    }

    #[test]
    fn any_pending_setup_entry_blocks_package_mutation_without_reading_or_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        require_complete(dir.path()).unwrap();
        let journal = dir.path().join(JOURNAL_NAME);
        std::fs::write(&journal, b"unparsed interrupted setup").unwrap();
        assert!(
            require_complete(dir.path())
                .unwrap_err()
                .to_string()
                .contains("matching installer")
        );
        assert_eq!(
            std::fs::read(&journal).unwrap(),
            b"unparsed interrupted setup"
        );
        std::fs::remove_file(&journal).unwrap();
        std::fs::create_dir(&journal).unwrap();
        assert!(require_complete(dir.path()).is_err());
        assert!(journal.is_dir());
    }
}
