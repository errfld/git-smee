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
        let mut cmd = Command::new(self.shell_program());
        cmd.arg(self.shell_flag());
        cmd
    }

    pub fn shell_program(&self) -> &'static str {
        match self {
            Platform::Windows => "cmd.exe",
            Platform::Unix => "sh",
        }
    }

    pub fn shell_flag(&self) -> &'static str {
        match self {
            Platform::Windows => "/C",
            Platform::Unix => "-c",
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
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn windows_make_executable_is_no_op() {
        let path = Path::new("this-file-does-not-need-to-exist");
        let result = Platform::Windows.make_executable(path);
        assert!(result.is_ok());
    }

    #[test]
    fn windows_command_uses_cmd_exe_and_c_flag() {
        let command = Platform::Windows.create_command();
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(command.get_program(), OsStr::new("cmd.exe"));
        assert_eq!(args, vec![OsStr::new("/C")]);
        assert_eq!(Platform::Windows.shell_program(), "cmd.exe");
    }

    #[test]
    fn unix_command_uses_sh_and_c_flag() {
        let command = Platform::Unix.create_command();
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(command.get_program(), OsStr::new("sh"));
        assert_eq!(args, vec![OsStr::new("-c")]);
        assert_eq!(Platform::Unix.shell_program(), "sh");
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
}
