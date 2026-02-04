use core::fmt;
use std::{
    collections::{HashMap, hash_map},
    fs,
    path::Path,
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Deserialize, Serialize)]
pub struct SmeeConfig {
    #[serde(flatten)]
    pub hooks: HashMap<LifeCyclePhase, Vec<HookDefinition>>,
}

impl SmeeConfig {
    /// Load configuration from a TOML file.
    ///
    /// Reads and parses the `.smee.toml` configuration file at the given path.
    /// The file must exists and have a `.toml` extension
    ///
    /// # Arguments
    ///
    /// * `path` - Paht to the TOML configuration file
    ///
    /// # Examples
    ///
    /// ```rust
    /// use git_smee_core::SmeeConfig;
    /// use git_smee_core::config::LifeCyclePhase;
    /// use std::fs;
    /// use tempfile::tempdir;
    ///
    /// let dir = tempdir().unwrap();
    /// let config_path = dir.path().join(".git-smee.toml");
    /// let toml_content = r#"
    /// [[pre-commit]]
    /// command = "cargo build"
    ///
    /// [[pre-commit]]
    /// command = "cargo test"
    /// "#;
    /// fs::write(&config_path, toml_content).unwrap();
    ///
    /// let config = SmeeConfig::from_toml(&config_path).unwrap();
    /// assert!(config.hooks.contains_key(&LifeCyclePhase::PreCommit));
    /// ```
    ///
    pub fn from_toml(path: &Path) -> Result<Self, Error> {
        if !path.exists() || !path.is_file() {
            return Err(Error::MissingFile);
        }
        let ext = path.extension().ok_or(Error::CanNotReadExtension)?;
        if ext != "toml" {
            return Err(Error::NotATomlFileExtension);
        }
        let data = fs::read(path).map_err(Error::ReadError)?;
        let config: SmeeConfig = toml::from_slice(&data).map_err(Error::ParseError)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        for (phase, hooks) in &self.hooks {
            if hooks.is_empty() {
                return Err(ValidationError::EmptyHookEntries {
                    hook_name: phase.to_string(),
                });
            }

            for (index, hook_definition) in hooks.iter().enumerate() {
                if hook_definition.command.trim().is_empty() {
                    return Err(ValidationError::EmptyCommand {
                        hook_name: phase.to_string(),
                        entry_index: index + 1,
                    });
                }
            }
        }

        Ok(())
    }
}

impl Default for SmeeConfig {
    fn default() -> Self {
        let mut hash_map: HashMap<LifeCyclePhase, Vec<HookDefinition>> = hash_map::HashMap::new();
        hash_map.insert(
            LifeCyclePhase::PreCommit,
            vec![HookDefinition {
                command: "echo 'Default pre-commit hook'".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        Self { hooks: hash_map }
    }
}

impl TryFrom<&Path> for SmeeConfig {
    type Error = Error;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        SmeeConfig::from_toml(value)
    }
}

impl TryFrom<&SmeeConfig> for String {
    type Error = Error;

    fn try_from(value: &SmeeConfig) -> Result<Self, Self::Error> {
        toml::to_string_pretty(value).map_err(Error::SerializeError)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDefinition {
    pub command: String,
    #[serde(default = "bool::default")]
    pub parallel_execution_allowed: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum LifeCyclePhase {
    ApplypatchMsg,
    PreApplypatch,
    PostApplypatch,
    PreCommit,
    PrepareCommitMsg,
    CommitMsg,
    PostCommit,
    PreMergeCommit,
    PreRebase,
    PostCheckout,
    PostMerge,
    PostRewrite,
    PrePush,
    ReferenceTransaction,
    PushToCheckout,
    PreAutoGc,
    PostUpdate,
    FsmonitorWatchman,
    PostIndexChange,
}

impl FromStr for LifeCyclePhase {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "applypatch-msg" => Ok(LifeCyclePhase::ApplypatchMsg),
            "pre-applypatch" => Ok(LifeCyclePhase::PreApplypatch),
            "post-applypatch" => Ok(LifeCyclePhase::PostApplypatch),
            "pre-commit" => Ok(LifeCyclePhase::PreCommit),
            "prepare-commit-msg" => Ok(LifeCyclePhase::PrepareCommitMsg),
            "commit-msg" => Ok(LifeCyclePhase::CommitMsg),
            "post-commit" => Ok(LifeCyclePhase::PostCommit),
            "pre-merge-commit" => Ok(LifeCyclePhase::PreMergeCommit),
            "pre-rebase" => Ok(LifeCyclePhase::PreRebase),
            "post-checkout" => Ok(LifeCyclePhase::PostCheckout),
            "post-merge" => Ok(LifeCyclePhase::PostMerge),
            "post-rewrite" => Ok(LifeCyclePhase::PostRewrite),
            "pre-push" => Ok(LifeCyclePhase::PrePush),
            "reference-transaction" => Ok(LifeCyclePhase::ReferenceTransaction),
            "push-to-checkout" => Ok(LifeCyclePhase::PushToCheckout),
            "pre-auto-gc" => Ok(LifeCyclePhase::PreAutoGc),
            "post-update" => Ok(LifeCyclePhase::PostUpdate),
            "fsmonitor-watchman" => Ok(LifeCyclePhase::FsmonitorWatchman),
            "post-index-change" => Ok(LifeCyclePhase::PostIndexChange),
            _ => Err(Error::UnknownLifeCyclePhase(s.to_string())),
        }
    }
}

impl fmt::Display for LifeCyclePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LifeCyclePhase::ApplypatchMsg => "applypatch-msg",
            LifeCyclePhase::PreApplypatch => "pre-applypatch",
            LifeCyclePhase::PostApplypatch => "post-applypatch",
            LifeCyclePhase::PreCommit => "pre-commit",
            LifeCyclePhase::PrepareCommitMsg => "prepare-commit-msg",
            LifeCyclePhase::CommitMsg => "commit-msg",
            LifeCyclePhase::PostCommit => "post-commit",
            LifeCyclePhase::PreMergeCommit => "pre-merge-commit",
            LifeCyclePhase::PreRebase => "pre-rebase",
            LifeCyclePhase::PostCheckout => "post-checkout",
            LifeCyclePhase::PostMerge => "post-merge",
            LifeCyclePhase::PostRewrite => "post-rewrite",
            LifeCyclePhase::PrePush => "pre-push",
            LifeCyclePhase::ReferenceTransaction => "reference-transaction",
            LifeCyclePhase::PushToCheckout => "push-to-checkout",
            LifeCyclePhase::PreAutoGc => "pre-auto-gc",
            LifeCyclePhase::PostUpdate => "post-update",
            LifeCyclePhase::FsmonitorWatchman => "fsmonitor-watchman",
            LifeCyclePhase::PostIndexChange => "post-index-change",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The specified configuration file is missing")]
    MissingFile,
    #[error("The specified configuration file does not have a readable extension")]
    CanNotReadExtension,
    #[error("The specified configuration file does not have a .toml extension")]
    NotATomlFileExtension,
    #[error("Failed to read the configuration file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Failed to parse the configuration file: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Failed to serialize the configuration: {0}")]
    SerializeError(#[from] toml::ser::Error),
    #[error("{0}")]
    ValidationError(#[from] ValidationError),
    #[error("Unknown lifecycle phase: {0}")]
    UnknownLifeCyclePhase(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Hook '{hook_name}' has no entries")]
    EmptyHookEntries { hook_name: String },
    #[error("Hook '{hook_name}' entry #{entry_index}: command must not be empty")]
    EmptyCommand {
        hook_name: String,
        entry_index: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = "
    [[pre-commit]]
    command = 'cargo build'

    [[pre-commit]]
    command = 'cargo test'
    ";

    #[test]
    fn test_create_from_toml() {
        let config: SmeeConfig = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(config.hooks.len(), 1);
        assert_eq!(config.hooks[&LifeCyclePhase::PreCommit].len(), 2);
        let hook_definition = config.hooks[&LifeCyclePhase::PreCommit]
            .first()
            .expect("Hook definition should be present");
        assert_eq!(hook_definition.command, "cargo build");
        assert!(!hook_definition.parallel_execution_allowed);
        let hook_definition = config.hooks[&LifeCyclePhase::PreCommit]
            .get(1)
            .expect("Second Hook Definition should be present");
        assert_eq!(hook_definition.command, "cargo test");
        assert!(!hook_definition.parallel_execution_allowed);
    }

    #[test]
    fn given_unknown_hook_key_when_deserializing_then_parse_error_contains_invalid_key() {
        let invalid_toml = r#"
        [[pre-commmit]]
        command = "cargo test"
        "#;

        let message = match toml::from_str::<SmeeConfig>(invalid_toml) {
            Ok(_) => panic!("expected parse error for unknown hook key"),
            Err(error) => error.to_string(),
        };

        assert!(message.contains("pre-commmit"));
    }

    #[test]
    fn given_multiple_unknown_hook_keys_when_deserializing_then_parse_fails_before_config_is_built()
    {
        let invalid_toml = r#"
        [[pre-commmit]]
        command = "cargo test"

        [[pre-puush]]
        command = "cargo fmt"
        "#;

        let message = match toml::from_str::<SmeeConfig>(invalid_toml) {
            Ok(_) => panic!("expected parse error for unknown hook keys"),
            Err(error) => error.to_string(),
        };

        assert!(message.contains("pre-commmit") || message.contains("pre-puush"));
    }

    #[test]
    fn given_default_config_when_try_into_string_then_string() {
        let config = SmeeConfig::default();
        assert_eq!(config.hooks.len(), 1);

        //when
        let serialized_config: String = (&config).try_into().unwrap();
        assert!(serialized_config.contains("pre-commit"))
    }

    #[test]
    fn given_lifecycle_when_from_str_then_correct_enum_returned() {
        let all_enums = [
            LifeCyclePhase::ApplypatchMsg,
            LifeCyclePhase::PreApplypatch,
            LifeCyclePhase::PostApplypatch,
            LifeCyclePhase::PreCommit,
            LifeCyclePhase::PrepareCommitMsg,
            LifeCyclePhase::CommitMsg,
            LifeCyclePhase::PostCommit,
            LifeCyclePhase::PreMergeCommit,
            LifeCyclePhase::PreRebase,
            LifeCyclePhase::PostCheckout,
            LifeCyclePhase::PostMerge,
            LifeCyclePhase::PostRewrite,
            LifeCyclePhase::PrePush,
            LifeCyclePhase::ReferenceTransaction,
            LifeCyclePhase::PushToCheckout,
            LifeCyclePhase::PreAutoGc,
            LifeCyclePhase::PostUpdate,
            LifeCyclePhase::FsmonitorWatchman,
            LifeCyclePhase::PostIndexChange,
        ];
        all_enums.iter().for_each(|phase| {
            let phase_str = phase.to_string();
            let parsed_phase = LifeCyclePhase::from_str(&phase_str).unwrap();
            assert_eq!(&parsed_phase, phase);
        });
    }

    #[test]
    fn given_empty_command_when_validating_then_error_contains_hook_and_entry() {
        let mut hooks = HashMap::new();
        hooks.insert(
            LifeCyclePhase::PreCommit,
            vec![
                HookDefinition {
                    command: "cargo test".to_string(),
                    parallel_execution_allowed: false,
                },
                HookDefinition {
                    command: "   ".to_string(),
                    parallel_execution_allowed: false,
                },
            ],
        );
        let config = SmeeConfig { hooks };

        let result = config.validate();

        assert_eq!(
            result,
            Err(ValidationError::EmptyCommand {
                hook_name: "pre-commit".to_string(),
                entry_index: 2,
            })
        );
    }

    #[test]
    fn given_hook_without_entries_when_validating_then_error_contains_hook() {
        let mut hooks = HashMap::new();
        hooks.insert(LifeCyclePhase::PrePush, vec![]);
        let config = SmeeConfig { hooks };

        let result = config.validate();

        assert_eq!(
            result,
            Err(ValidationError::EmptyHookEntries {
                hook_name: "pre-push".to_string(),
            })
        );
    }

    #[test]
    fn given_valid_config_when_validating_then_success() {
        let mut hooks = HashMap::new();
        hooks.insert(
            LifeCyclePhase::PreCommit,
            vec![HookDefinition {
                command: "cargo test".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks };

        assert!(config.validate().is_ok());
    }
}
