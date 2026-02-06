use std::{path::Path, process::Command};

#[cfg(any(unix, test))]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use thiserror::Error;

#[derive(Debug, PartialEq)]
pub enum Platform {
    Unix,
    Windows,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to get metadata for path: {0}")]
    FailedToGetMetadata(std::io::Error),
    #[error("Failed to set permissions for path: {0}")]
    FailedToSetPermissions(std::io::Error),
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        }
    }

    pub fn hook_script_template(&self) -> &'static str {
        match self {
            Platform::Unix => include_str!("scripts/hook_template_unix"),
            Platform::Windows => include_str!("scripts/hook_template_windows"),
        }
    }

    pub fn make_executable(&self, hook_path: &Path) -> Result<(), Error> {
        match self {
            Platform::Windows => Ok(()),
            Platform::Unix => make_executable_unix(hook_path),
        }
    }

    pub fn create_command(&self) -> Command {
        match self {
            Platform::Windows => {
                let mut cmd = Command::new("cmd.exe");
                cmd.arg("/C");
                cmd
            }
            Platform::Unix => {
                let mut cmd = Command::new("sh");
                cmd.arg("-c");
                cmd
            }
        }
    }

    pub fn shell_display(&self) -> &'static str {
        match self {
            Platform::Windows => "cmd.exe /C",
            Platform::Unix => "sh -c",
        }
    }
}

#[cfg(unix)]
fn make_executable_unix(hook_path: &Path) -> Result<(), Error> {
    let metadata = fs::metadata(hook_path).map_err(Error::FailedToGetMetadata)?;
    let permissions = metadata.permissions().mode() | 0o111;
    fs::set_permissions(hook_path, fs::Permissions::from_mode(permissions))
        .map_err(Error::FailedToSetPermissions)
}

#[cfg(not(unix))]
fn make_executable_unix(_hook_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_make_executable_is_no_op() {
        let path = Path::new("this-file-does-not-need-to-exist");
        let result = Platform::Windows.make_executable(path);
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn unix_make_executable_adds_execute_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let hook_path = temp_dir.path().join("pre-commit");
        fs::write(&hook_path, "#!/usr/bin/env sh\necho test\n").unwrap();
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o640)).unwrap();

        Platform::Unix.make_executable(&hook_path).unwrap();

        let mode = fs::metadata(&hook_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[cfg(not(unix))]
    #[test]
    fn unix_make_executable_falls_back_to_no_op_on_non_unix() {
        let path = Path::new("does-not-exist-on-purpose");
        let result = Platform::Unix.make_executable(path);
        assert!(result.is_ok());
    }

    #[test]
    fn windows_create_command_uses_cmd_exe_with_c_flag() {
        let cmd = Platform::Windows.create_command();
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(cmd.get_program().to_string_lossy(), "cmd.exe");
        assert_eq!(args, vec!["/C"]);
        assert_eq!(Platform::Windows.shell_display(), "cmd.exe /C");
    }

    #[test]
    fn unix_create_command_uses_portable_sh_with_c_flag() {
        let cmd = Platform::Unix.create_command();
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(cmd.get_program().to_string_lossy(), "sh");
        assert_eq!(args, vec!["-c"]);
        assert_eq!(Platform::Unix.shell_display(), "sh -c");
    }
}
