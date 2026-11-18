//! Optional hard CPU-rate ceiling for one owned Windows process job.
//! Not node-wide allocation, GPU control, reserved capacity or a sandbox.
use anyhow::{Result, bail};

pub(crate) fn validate(percent: Option<u32>) -> Result<()> {
    if percent.is_some_and(|value| !(1..=100).contains(&value)) {
        bail!("Owned-job CPU ceiling must be 1-100 percent")
    }
    Ok(())
}

pub(crate) fn supported(percent: Option<u32>) -> Result<()> {
    validate(percent)?;
    if percent.is_some() && !cfg!(windows) {
        bail!("Owned-job CPU rate control is supported only on Windows; no unlimited fallback")
    }
    Ok(())
}

pub(crate) fn native_rate(percent: Option<u32>) -> Result<Option<u32>> {
    supported(percent)?;
    Ok(percent.map(|value| value * 100))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_ceiling_bounds_and_platform_refusal_are_explicit() {
        for value in [None, Some(1), Some(25), Some(100)] {
            validate(value).unwrap();
        }
        for value in [0, 101, u32::MAX] {
            assert!(native_rate(Some(value)).is_err());
        }
        assert_eq!(native_rate(None).unwrap(), None);
        #[cfg(windows)]
        for (value, rate) in [(1, 100), (25, 2500), (100, 10000)] {
            assert_eq!(native_rate(Some(value)).unwrap(), Some(rate));
        }
        #[cfg(not(windows))]
        assert!(native_rate(Some(25)).is_err());
    }
}
