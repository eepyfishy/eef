//! Shared same-directory synchronized staging followed by atomic file replacement.
//! There is deliberately no copy-over-live-file fallback. This is not a
//! multi-file transaction or a power-loss durability guarantee for directories.
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

fn replace_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    if path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected a destination file",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    // Unique create-new files cannot truncate legacy .next siblings. Staging in
    // the destination directory also keeps persistence on the same filesystem.
    let mut pending = tempfile::Builder::new()
        .prefix(".eef-write-")
        .tempfile_in(parent)?;
    write(pending.as_file_mut())?;
    pending.as_file().sync_all()?;
    // On failure PersistError owns the staging file; dropping it cleans up only
    // that new file. Never fall back to modifying the destination in place.
    drop(pending.persist(path).map_err(|failure| failure.error)?);
    Ok(())
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, |file| file.write_all(bytes))
}

pub fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source = File::open(source)?;
    replace_with(destination, |file| io::copy(&mut source, file).map(|_| ()))
}

/// Publish a complete new file, never replacing a concurrently created target.
pub fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
    create_with(path, |file| file.write_all(bytes))
}

fn create_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    if path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected a destination file",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut pending = tempfile::Builder::new()
        .prefix(".eef-write-")
        .tempfile_in(parent)?;
    write(pending.as_file_mut())?;
    pending.as_file().sync_all()?;
    drop(
        pending
            .persist_noclobber(path)
            .map_err(|failure| failure.error)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staging_files(parent: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".eef-write-")
            })
            .collect()
    }

    #[test]
    fn atomic_creation_never_replaces_existing_owner_bytes_or_directories() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/settings.json");
        create(&target, b"complete owner settings").unwrap();
        assert!(create(&target, b"replacement").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"complete owner settings");
        assert!(create(target.parent().unwrap(), b"not a directory").is_err());
        assert!(staging_files(target.parent().unwrap()).is_empty());
    }

    #[test]
    fn atomic_creation_has_exactly_one_winner_for_the_same_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for bytes in [
            b"first complete settings".to_vec(),
            b"second complete settings".to_vec(),
        ] {
            let barrier = barrier.clone();
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let result = create(&path, &bytes);
                (bytes, result)
            }));
        }
        barrier.wait();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|(_, r)| r.is_ok()).count(), 1);
        let winner = results.iter().find(|(_, r)| r.is_ok()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), winner.0);
        assert!(staging_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_creation_partial_write_failure_never_publishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new-settings");
        let failure = create_with(&path, |file| {
            file.write_all(b"partial settings")?;
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected disk failure",
            ))
        });
        assert!(failure.is_err());
        assert!(!path.exists());
        assert!(staging_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_creation_preserves_owner_file_created_during_staging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings");
        let other = path.clone();
        let (ready, staged) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            create_with(&other, |file| {
                file.write_all(b"losing staged settings")?;
                ready.send(()).unwrap();
                resume
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Ok(())
            })
        });
        staged
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        write(&path, b"concurrently published owner settings").unwrap();
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"concurrently published owner settings"
        );
        assert!(staging_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_settings_shared_writer_replaces_whole_files_and_copies_backups() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/settings.json");
        write(&target, b"first settings").unwrap();
        let backup = target.with_extension("json.bak");
        copy(&target, &backup).unwrap();
        write(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"first settings");
        copy(&target, &backup).unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), b"new");
        assert!(staging_files(target.parent().unwrap()).is_empty());
    }

    #[test]
    fn atomic_settings_partial_write_failure_never_publishes_or_deletes_other_files() {
        for existing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("settings.json");
            if existing {
                std::fs::write(&target, b"old intact settings").unwrap();
            }
            let sibling = dir.path().join("settings.json.next");
            std::fs::write(&sibling, b"unrelated sibling").unwrap();
            let failure = replace_with(&target, |file| {
                file.write_all(b"partial new settings")?;
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Injected partial write failure",
                ))
            });
            assert!(failure.is_err());
            if existing {
                assert_eq!(std::fs::read(&target).unwrap(), b"old intact settings");
            } else {
                assert!(!target.exists());
            }
            assert_eq!(std::fs::read(sibling).unwrap(), b"unrelated sibling");
            assert!(staging_files(dir.path()).is_empty());
        }
    }

    #[test]
    fn atomic_settings_distinct_concurrent_destinations_have_distinct_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let (ready, received) = std::sync::mpsc::channel();
        let mut threads = Vec::new();
        for name in ["node.json", "node.yaml"] {
            let path = dir.path().join(name);
            let barrier = barrier.clone();
            let ready = ready.clone();
            threads.push(std::thread::spawn(move || {
                replace_with(&path, |file| {
                    file.write_all(name.as_bytes())?;
                    ready.send(()).unwrap();
                    barrier.wait();
                    Ok(())
                })
                .unwrap();
                assert_eq!(std::fs::read(path).unwrap(), name.as_bytes());
            }));
        }
        for _ in 0..2 {
            received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        let pending = staging_files(dir.path());
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(pending.len(), 2);
        assert_ne!(pending[0], pending[1]);
        assert!(staging_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_settings_missing_copy_source_preserves_backup_without_staging() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("settings.bak");
        std::fs::write(&backup, b"last valid backup").unwrap();
        assert!(copy(&dir.path().join("missing"), &backup).is_err());
        assert_eq!(std::fs::read(backup).unwrap(), b"last valid backup");
        assert!(staging_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_settings_failed_publication_preserves_destination_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing-directory");
        std::fs::create_dir(&target).unwrap();
        let marker = target.join("user-data");
        std::fs::write(&marker, b"keep this").unwrap();
        assert!(write(&target, b"must not replace a directory").is_err());
        assert_eq!(std::fs::read(marker).unwrap(), b"keep this");
        assert!(staging_files(dir.path()).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn atomic_settings_backup_replacement_refuses_a_delete_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("settings.json");
        let backup = dir.path().join("settings.json.bak");
        std::fs::write(&source, b"current settings").unwrap();
        std::fs::write(&backup, b"previous backup").unwrap();
        let _guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&backup)
            .unwrap();
        assert!(copy(&source, &backup).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"current settings");
        assert_eq!(std::fs::read(backup).unwrap(), b"previous backup");
        assert!(staging_files(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_settings_new_files_are_owner_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.json");
        write(&target, b"private").unwrap();
        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
