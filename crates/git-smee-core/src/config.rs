use core::fmt;
use std::{
    collections::{HashMap, hash_map},
    ffi::OsStr,
    fs,
    path::Path,
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

#[derive(Serialize)]
pub struct SmeeConfig {
    #[serde(flatten)]
    pub hooks: HashMap<LifeCyclePhase, Vec<HookDefinition>>,
}

#[derive(Deserialize)]
struct SmeeConfigWire {
    #[serde(flatten)]
    hooks: HashMap<LifeCyclePhase, Vec<HookDefinitionWire>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HookDefinitionWire {
    command: String,
    #[serde(default = "bool::default")]
    parallel_execution_allowed: bool,
}

impl<'de> Deserialize<'de> for SmeeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SmeeConfigWire::deserialize(deserializer)?;
        let hooks = wire
            .hooks
            .into_iter()
            .map(|(phase, definitions)| {
                let definitions = definitions
                    .into_iter()
                    .enumerate()
                    .map(|(index, definition)| {
                        let command = HookCommand::try_from(definition.command).map_err(|_| {
                            de::Error::custom(ValidationError::EmptyCommand {
                                hook_name: phase.to_string(),
                                entry_index: index + 1,
                            })
                        })?;
                        Ok(HookDefinition {
                            command,
                            parallel_execution_allowed: definition.parallel_execution_allowed,
                        })
                    })
                    .collect::<Result<Vec<_>, D::Error>>()?;
                Ok((phase, definitions))
            })
            .collect::<Result<HashMap<_, _>, D::Error>>()?;
        Ok(Self { hooks })
    }
}

impl SmeeConfig {
    /// Load configuration from a TOML file.
    ///
    /// Reads and parses the `.smee.toml` configuration file at the given path.
    /// The file must exists and have a `.toml` extension
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
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
        if !path.exists() {
            return Err(Error::MissingFile);
        }
        if !path.is_file() {
            return Err(Error::NotAFile);
        }
        let ext = path.extension().ok_or(Error::CanNotReadExtension)?;
        if !ext.eq_ignore_ascii_case(OsStr::new("toml")) {
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
                command: HookCommand::try_from("echo 'Default pre-commit hook'")
                    .expect("default hook command should be valid"),
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
    pub command: HookCommand,
    #[serde(default = "bool::default")]
    pub parallel_execution_allowed: bool,
}

/// A validated, non-empty shell command configured for a Git hook.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HookCommand(String);

impl HookCommand {
    /// Returns the exact configured shell source without trimming or normalization.
    pub fn as_shell_source(&self) -> &str {
        &self.0
    }

    pub(crate) fn redacted(&self) -> String {
        crate::executor::redaction::redact_command(self.as_shell_source())
    }
}

impl TryFrom<String> for HookCommand {
    type Error = HookCommandError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err(HookCommandError)
        } else {
            Ok(Self(value))
        }
    }
}

impl TryFrom<&str> for HookCommand {
    type Error = HookCommandError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl<'de> Deserialize<'de> for HookCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        Self::try_from(source).map_err(de::Error::custom)
    }
}

/// Returned when a hook command contains only whitespace or no characters.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("command must not be empty")]
pub struct HookCommandError;

impl PartialEq<&str> for HookCommand {
    fn eq(&self, other: &&str) -> bool {
        self.as_shell_source() == *other
    }
}

impl PartialEq<HookCommand> for &str {
    fn eq(&self, other: &HookCommand) -> bool {
        *self == other.as_shell_source()
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone, Copy)]
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
    PreReceive,
    Update,
    ProcReceive,
    PostReceive,
    ReferenceTransaction,
    PushToCheckout,
    PreAutoGc,
    PostUpdate,
    FsmonitorWatchman,
    PostIndexChange,
}

const ALL_LIFECYCLE_PHASES: [LifeCyclePhase; 23] = [
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
    LifeCyclePhase::PreReceive,
    LifeCyclePhase::Update,
    LifeCyclePhase::ProcReceive,
    LifeCyclePhase::PostReceive,
    LifeCyclePhase::ReferenceTransaction,
    LifeCyclePhase::PushToCheckout,
    LifeCyclePhase::PreAutoGc,
    LifeCyclePhase::PostUpdate,
    LifeCyclePhase::FsmonitorWatchman,
    LifeCyclePhase::PostIndexChange,
];

impl LifeCyclePhase {
    pub const fn all() -> &'static [LifeCyclePhase] {
        &ALL_LIFECYCLE_PHASES
    }

    pub const fn as_str(self) -> &'static str {
        match self {
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
            LifeCyclePhase::PreReceive => "pre-receive",
            LifeCyclePhase::Update => "update",
            LifeCyclePhase::ProcReceive => "proc-receive",
            LifeCyclePhase::PostReceive => "post-receive",
            LifeCyclePhase::ReferenceTransaction => "reference-transaction",
            LifeCyclePhase::PushToCheckout => "push-to-checkout",
            LifeCyclePhase::PreAutoGc => "pre-auto-gc",
            LifeCyclePhase::PostUpdate => "post-update",
            LifeCyclePhase::FsmonitorWatchman => "fsmonitor-watchman",
            LifeCyclePhase::PostIndexChange => "post-index-change",
        }
    }
}

impl FromStr for LifeCyclePhase {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::all()
            .iter()
            .copied()
            .find(|phase| phase.as_str() == s)
            .ok_or_else(|| Error::UnknownLifeCyclePhase(s.to_string()))
    }
}

impl fmt::Display for LifeCyclePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The specified configuration file is missing")]
    MissingFile,
    #[error("The specified configuration path exists but is not a regular file")]
    NotAFile,
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
    use std::fs;

    use tempfile::tempdir;

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
    fn given_uppercase_toml_extension_when_loading_then_config_is_accepted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".git-smee.TOML");
        fs::write(&path, EXAMPLE_TOML).unwrap();

        let result = SmeeConfig::from_toml(&path);

        assert!(result.is_ok());
    }

    #[test]
    fn given_mixed_case_toml_extension_when_loading_then_config_is_accepted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".git-smee.ToMl");
        fs::write(&path, EXAMPLE_TOML).unwrap();

        let result = SmeeConfig::from_toml(&path);

        assert!(result.is_ok());
    }

    #[test]
    fn given_non_toml_extension_when_loading_then_error_is_returned() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".git-smee.yaml");
        fs::write(&path, EXAMPLE_TOML).unwrap();

        let result = SmeeConfig::from_toml(&path);

        assert!(matches!(result, Err(Error::NotATomlFileExtension)));
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
        LifeCyclePhase::all().iter().for_each(|phase| {
            let phase_str = phase.to_string();
            let parsed_phase = LifeCyclePhase::from_str(&phase_str).unwrap();
            assert_eq!(&parsed_phase, phase);
        });
    }

    #[test]
    fn given_supported_hooks_table_when_checking_readme_then_it_matches_runtime_supported_hooks() {
        let readme_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("README.md");
        let readme = fs::read_to_string(readme_path).unwrap();
        let table = readme
            .split("### Supported Git Hooks")
            .nth(1)
            .and_then(|section| section.split("### Hook argument forwarding").next())
            .expect("README supported hooks section should exist");
        let documented_hooks: Vec<String> = table
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.starts_with('|')
                    || trimmed.starts_with("| Hook")
                    || trimmed.starts_with("|------")
                {
                    return None;
                }
                trimmed
                    .split('|')
                    .nth(1)
                    .map(str::trim)
                    .filter(|hook| !hook.is_empty())
                    .map(|hook| hook.trim_matches('`').to_owned())
            })
            .collect();
        let runtime_hooks: Vec<String> = LifeCyclePhase::all()
            .iter()
            .map(|phase| phase.to_string())
            .collect();

        assert_eq!(documented_hooks, runtime_hooks);
    }

    #[test]
    fn given_hook_command_when_inspecting_then_shell_source_and_redacted_display_are_owned_by_type()
    {
        let command = HookCommand::try_from("TOKEN=super-secret deploy --token hidden").unwrap();

        assert_eq!(
            command.as_shell_source(),
            "TOKEN=super-secret deploy --token hidden"
        );
        assert_eq!(command.redacted(), "deploy <args redacted>");
    }

    #[test]
    fn given_empty_or_whitespace_hook_command_when_constructing_then_command_type_rejects_it() {
        assert!(HookCommand::try_from("").is_err());
        assert!(HookCommand::try_from("   \t\n").is_err());
        assert!(HookCommand::try_from(String::new()).is_err());
    }

    #[test]
    fn given_padded_non_empty_hook_command_when_constructing_then_exact_shell_source_is_preserved()
    {
        let command = HookCommand::try_from("  echo preserved  ").unwrap();

        assert_eq!(command.as_shell_source(), "  echo preserved  ");
    }

    #[test]
    fn given_empty_or_whitespace_hook_command_when_deserializing_then_it_is_rejected() {
        for source in ["", "   \t"] {
            let config = format!(
                r#"
            [[pre-commit]]
            command = "{source}"
            "#
            );

            assert!(toml::from_str::<SmeeConfig>(&config).is_err());
        }
    }

    #[test]
    fn given_padded_hook_command_when_deserializing_then_exact_shell_source_is_preserved() {
        let config = toml::from_str::<SmeeConfig>(
            r#"
            [[pre-commit]]
            command = "  echo preserved  "
            "#,
        )
        .unwrap();

        assert_eq!(
            config.hooks[&LifeCyclePhase::PreCommit][0]
                .command
                .as_shell_source(),
            "  echo preserved  "
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
                command: "cargo test".try_into().unwrap(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks };

        assert!(config.validate().is_ok());
    }
}
