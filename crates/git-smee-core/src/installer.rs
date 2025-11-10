use crate::SmeeConfig;
use std::{fs, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Not implemented yet")]
    NotImplemented,
    #[error("Hooks directory not found: {0}")]
    HooksDirNotFound(String),
    #[error("No hooks present in the configuration to install")]
    NoHooksPresent,
    #[error("Failed to write hook: {0}")]
    FailedToWriteHook(#[from] std::io::Error),
    // add installer-specific errors here later
}

pub trait HookInstaller {
    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<(), Error>;
}

pub struct FileSystemHookInstaller {
    hooks_path: PathBuf,
}

impl FileSystemHookInstaller {
    const HOOKS_DIR: &str = ".git/hooks";
    pub fn from_default() -> Result<Self, Error> {
        Self::from_path(PathBuf::from(Self::HOOKS_DIR))
    }

    pub fn from_path(hooks_path: PathBuf) -> Result<Self, Error> {
        if !hooks_path.exists() || !hooks_path.is_dir() {
            return Err(Error::HooksDirNotFound(
                hooks_path.to_string_lossy().to_string(),
            ));
        }

        Ok(Self { hooks_path })
    }
}

impl HookInstaller for FileSystemHookInstaller {
    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<(), Error> {
        let hook_file = self.hooks_path.join(hook_name);
        fs::write(hook_file, hook_content).map_err(Error::FailedToWriteHook)
    }
}

pub fn install_hooks<T: HookInstaller>(
    config: &SmeeConfig,
    hook_installer: &T,
) -> Result<(), Error> {
    if config.hooks.is_empty() {
        return Err(Error::NoHooksPresent);
    }
    config
        .hooks
        .keys()
        .map(|life_cycle_phase| {
            let lifecycle_phase_kebap = life_cycle_phase.to_string();
            let content = HOOK_TEMPLATE.replace("{hook}", &lifecycle_phase_kebap);
            hook_installer.install_hook(&lifecycle_phase_kebap, &content)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(())
}

const HOOK_TEMPLATE: &str = r#"#!/usr/bin/env sh
#DO NOT MODIFY THIS FILE DIRECTLY
#THIS FILE IS MANAGED BY GIT-SMEE
  set -e
  git smee run {hook}
  "#;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use super::*;

    struct AssertingHookInstaller {
        assertion: fn(hook_name: &str, hook_content: &str) -> (),
        number_of_installed_hooks: AtomicU8,
    }

    impl HookInstaller for AssertingHookInstaller {
        fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<(), Error> {
            (self.assertion)(hook_name, hook_content);
            self.number_of_installed_hooks
                .fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn given_empty_smee_config_when_installing_hooks_then_no_hooks_present_error() {
        let config = SmeeConfig {
            hooks: std::collections::HashMap::new(),
        };

        let installer = AssertingHookInstaller {
            assertion: |_, _| panic!("No hooks should be installed"),
            number_of_installed_hooks: AtomicU8::new(0),
        };

        let result = install_hooks(&config, &installer);
        assert!(matches!(result, Err(Error::NoHooksPresent)));
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn given_single_hook_when_installing_hooks_then_hook_installed() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };

        let installer = AssertingHookInstaller {
            assertion: |hook_name, hook_content| {
                assert_eq!(hook_name, "pre-commit");
                assert!(hook_content.contains("git smee run pre-commit"));
            },
            number_of_installed_hooks: AtomicU8::new(0),
        };

        let result = install_hooks(&config, &installer);
        assert!(result.is_ok());
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn given_multiple_hooks_when_installing_hooks_then_all_hooks_installed() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        hooks_map.insert(
            crate::config::LifeCyclePhase::PrePush,
            vec![crate::config::HookDefinition {
                command: "echo Pre-push hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let installer = AssertingHookInstaller {
            assertion: |hook_name, hook_content| match hook_name {
                "pre-commit" => {
                    assert!(hook_content.contains("git smee run pre-commit"));
                }
                "pre-push" => {
                    assert!(hook_content.contains("git smee run pre-push"));
                }
                _ => panic!("Unexpected hook name: {hook_name}"),
            },
            number_of_installed_hooks: AtomicU8::new(0),
        };
        let result = install_hooks(&config, &installer);
        assert!(result.is_ok());
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            2
        );
    }
}
