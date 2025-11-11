use std::{fs, os::unix::fs::PermissionsExt, path::Path};

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
            Platform::Unix => {
                let metadata = fs::metadata(hook_path).map_err(Error::FailedToGetMetadata)?;
                let permissions = metadata.permissions().mode() | 0o111;
                fs::set_permissions(hook_path, fs::Permissions::from_mode(permissions))
                    .map_err(Error::FailedToSetPermissions)
            }
        }
    }
}
