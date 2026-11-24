//! A pending EXE setup must recover before another package mutation.
use anyhow::{Context, Result, bail};
use std::path::Path;

pub const JOURNAL_NAME: &str = ".setup-transaction";

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
