//! Optional owned-model Windows job-commit ceiling, not working-set/RAM/VRAM.
use anyhow::{Result, bail};
pub const MIN_JOB_MEMORY_MIB: u64 = 64;
pub const MAX_JOB_MEMORY_MIB: u64 = 1_048_576;
pub fn validate(limit: Option<u64>) -> Result<()> {
    if limit.is_some_and(|mib| !(MIN_JOB_MEMORY_MIB..=MAX_JOB_MEMORY_MIB).contains(&mib)) {
        bail!("Owned-model job memory limit must be 64-1048576 MiB, or explicit default")
    }
    Ok(())
}
pub(crate) fn supported(limit: Option<u64>) -> Result<()> {
    validate(limit)?;
    #[cfg(not(windows))]
    if limit.is_some() {
        bail!("Owned-model job memory limits are supported only on Windows; no unlimited fallback")
    }
    Ok(())
}
pub(crate) fn bytes(limit: Option<u64>) -> Result<Option<usize>> {
    supported(limit)?;
    limit
        .map(|mib| {
            usize::try_from(
                mib.checked_mul(1024 * 1024)
                    .ok_or_else(|| anyhow::anyhow!("Job memory limit overflow"))?,
            )
            .map_err(|_| anyhow::anyhow!("Job memory limit is not representable on this platform"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_commit_bounds_and_platform_support_are_explicit() {
        for limit in [None, Some(64), Some(128), Some(MAX_JOB_MEMORY_MIB)] {
            validate(limit).unwrap();
        }
        for limit in [0, 63, MAX_JOB_MEMORY_MIB + 1, u64::MAX] {
            assert!(validate(Some(limit)).is_err());
        }
        assert_eq!(bytes(None).unwrap(), None);
        #[cfg(windows)]
        assert_eq!(bytes(Some(128)).unwrap(), Some(128 * 1024 * 1024));
        #[cfg(not(windows))]
        assert!(bytes(Some(128)).is_err());
    }
}
