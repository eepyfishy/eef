//! Selected application bundle versus persistent installation/configuration data.
//! A bundle marker identifies lookup policy only; it is not signature/hash trust.
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

#[derive(Clone)]
pub struct RuntimeLayout {
    installation: PathBuf,
    bundle: PathBuf,
    managed: bool,
}
impl RuntimeLayout {
    pub fn current() -> Result<Self> {
        Self::from_executable(std::env::current_exe()?)
    }
    pub fn from_executable(executable: impl AsRef<Path>) -> Result<Self> {
        let executable = executable.as_ref().canonicalize()?;
        let installation = crate::updater::installation_root(&executable)?;
        let bundle = executable
            .parent()
            .context("Missing executable directory")?
            .to_path_buf();
        let marker = match std::fs::symlink_metadata(bundle.join("bundle.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            // Malformed/unreadable markers must not enable implicit fallback.
            _ => true,
        };
        let managed = bundle != installation || marker;
        Ok(Self {
            installation,
            bundle,
            managed,
        })
    }
    pub fn model_binary(&self, config: &Path, value: &Path, bundled_default: &Path) -> PathBuf {
        if value.is_absolute() {
            return value.to_path_buf();
        }
        let configured = config_path(config, value);
        let default = value == bundled_default;
        if default && self.managed && same_directory(config_parent(config), &self.installation) {
            return self.bundle.join(value);
        }
        // A configuration outside the installation can retain its own relative
        // executable. Non-default relative and absolute choices remain explicit.
        if configured.is_file() {
            return configured;
        }
        if default {
            self.bundle.join(value)
        } else {
            self.installation.join(value)
        }
    }
    pub fn builtin_plugin(
        &self,
        filename: &str,
        development_root: Option<&Path>,
    ) -> Result<PathBuf> {
        let mut parts = Path::new(filename).components();
        if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
            bail!("Built-in plugin must be one filename")
        }
        let bundled = self.bundle.join("python/plugins").join(filename);
        if bundled.is_file() {
            return Ok(bundled);
        }
        if !self.managed {
            if let Some(candidate) = development_root
                .map(|root| root.join("python/plugins").join(filename))
                .filter(|p| p.is_file())
            {
                return Ok(candidate);
            }
        }
        bail!("Enabled built-in plugin '{filename}' is missing from this application bundle")
    }
    /// Managed bundles return their expected path even when missing, so probing
    /// fails there rather than executing an unrelated interpreter from PATH.
    pub fn bundled_python(&self) -> Option<PathBuf> {
        let candidates = if cfg!(windows) {
            vec![self.bundle.join("python/python.exe")]
        } else {
            vec![
                self.bundle.join("python/bin/python3"),
                self.bundle.join("python/bin/python"),
            ]
        };
        candidates
            .iter()
            .find(|path| path.is_file())
            .cloned()
            .or_else(|| self.managed.then(|| candidates[0].clone()))
    }

    /// Public publisher metadata belongs to the running bundle, never a pending
    /// selection, an old root or the current working directory. Not authority.
    pub(crate) fn repair_source_hint(&self, program: &str) -> Result<Option<PathBuf>> {
        if !matches!(program, "eef" | "eefn") {
            bail!("Unsupported repair product")
        }
        Ok(self
            .managed
            .then(|| self.bundle.join(format!("{program}.repair-source.json"))))
    }
}

pub fn config_path(config: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        config_parent(config).join(value)
    }
}
fn config_parent(config: &Path) -> &Path {
    config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}
fn same_directory(a: &Path, b: &Path) -> bool {
    a.canonicalize().unwrap_or_else(|_| a.to_path_buf())
        == b.canonicalize().unwrap_or_else(|_| b.to_path_buf())
}

#[cfg(test)]
#[path = "runtime_layout_tests.rs"]
mod tests;
