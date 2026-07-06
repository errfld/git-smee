use std::path::{Path, PathBuf};

use crate::{DEFAULT_CONFIG_FILE_NAME, platform::Platform};

use super::Error;

#[derive(Debug, Clone)]
pub struct HookScriptOptions {
    pub git_smee_executable: PathBuf,
    pub config_path: PathBuf,
}

impl HookScriptOptions {
    pub fn new(git_smee_executable: PathBuf, config_path: PathBuf) -> Self {
        Self {
            git_smee_executable,
            config_path,
        }
    }

    pub(crate) fn default_for_runtime() -> Result<Self, Error> {
        Ok(Self {
            git_smee_executable: std::env::current_exe()
                .map_err(Error::FailedToResolveCurrentExecutable)?,
            config_path: PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
        })
    }
}

pub(crate) fn render_hook_script(
    platform: &Platform,
    hook_name: &str,
    options: &HookScriptOptions,
) -> String {
    let escaped_executable = shell_single_quote(&options.git_smee_executable);
    let escaped_config_path = shell_single_quote(&options.config_path);
    platform
        .hook_script_template()
        .replace("{hook}", hook_name)
        .replace("{git_smee_executable}", &escaped_executable)
        .replace("{config_path}", &escaped_config_path)
}

pub(crate) fn shell_single_quote(path: &Path) -> String {
    unix_shell_path_word(path)
}

#[cfg(unix)]
fn unix_shell_path_word(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    match path.as_os_str().to_str() {
        Some(path) => format!("'{}'", path.replace('\'', "'\"'\"'")),
        None => {
            let escaped = path
                .as_os_str()
                .as_bytes()
                .iter()
                .map(|byte| format!(r"\{byte:03o}"))
                .collect::<String>();
            format!(r#"$(printf '%b' '{escaped}')"#)
        }
    }
}

#[cfg(not(unix))]
fn unix_shell_path_word(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}
