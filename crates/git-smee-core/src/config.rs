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
    pub fn from_toml(path: &Path) -> Result<Self, Error> {
        if !path.exists() || !path.is_file() {
            return Err(Error::MissingFile);
        }
        let ext = path.extension().ok_or(Error::CanNotReadExtension)?;
        if ext != "toml" {
            return Err(Error::NotATomlFileExtension);
        }
        let data = fs::read(path).map_err(Error::ReadError)?;
        toml::from_slice(&data).map_err(Error::ParseError)
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

    #[serde(other)]
    Unknown,
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
            LifeCyclePhase::Unknown => "unknown",
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
    #[error("Configuration validation error")]
    ValidationError,
    #[error("Unknown lifecycle phase: {0}")]
    UnknownLifeCyclePhase(String),
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
}
