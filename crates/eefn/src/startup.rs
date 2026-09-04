use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct StartupStatus {
    pub supported: bool,
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

pub fn startup_status(name: &str) -> Result<StartupStatus> {
    let Some(path) = startup_path(name)? else {
        return Ok(StartupStatus {
            supported: false,
            enabled: false,
            path: None,
        });
    };
    Ok(StartupStatus {
        supported: true,
        enabled: path.is_file(),
        path: Some(path),
    })
}

pub fn set_startup(
    name: &str,
    executable: &Path,
    arguments: &[String],
    enabled: bool,
) -> Result<StartupStatus> {
    let Some(path) = startup_path(name)? else {
        bail!("startup applications are not supported on this platform")
    };
    if enabled {
        let executable = executable
            .canonicalize()
            .context("executable does not exist")?;
        reject_cmd_metacharacters(&executable.to_string_lossy())?;
        for argument in arguments {
            reject_cmd_metacharacters(argument)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let args = arguments
            .iter()
            .map(|argument| format!("\"{argument}\""))
            .collect::<Vec<_>>()
            .join(" ");
        std::fs::write(
            &path,
            format!(
                "@echo off\r\nstart \"\" /min \"{}\" {}\r\n",
                executable.display(),
                args
            ),
        )?;
    } else if path.is_file() {
        std::fs::remove_file(&path)?;
    }
    startup_status(name)
}

fn reject_cmd_metacharacters(value: &str) -> Result<()> {
    if value.contains(['\r', '\n', '"', '&', '|', '<', '>', '^']) {
        bail!("path or argument contains unsupported command characters")
    }
    Ok(())
}

fn startup_path(name: &str) -> Result<Option<PathBuf>> {
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_'))
    {
        bail!("unsafe startup application name")
    }
    if !cfg!(windows) {
        return Ok(None);
    }
    let app_data = std::env::var_os("APPDATA").context("APPDATA is unavailable")?;
    Ok(Some(
        PathBuf::from(app_data)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join(format!("{name}.cmd")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_startup_names_and_arguments() {
        assert!(startup_status("../bad").is_err());
        assert!(reject_cmd_metacharacters("ok").is_ok());
        assert!(reject_cmd_metacharacters("bad & command").is_err());
    }
}
