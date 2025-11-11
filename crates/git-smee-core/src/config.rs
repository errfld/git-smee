use core::fmt;
use std::{collections::HashMap, fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

#[derive(Deserialize)]
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

impl TryFrom<&Path> for SmeeConfig {
    type Error = Error;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        SmeeConfig::from_toml(value)
    }
}

#[derive(Deserialize)]
pub struct HookDefinition {
    pub command: String,
    #[serde(default = "bool::default")]
    pub parallel_execution_allowed: bool,
}

#[derive(Deserialize, PartialEq, Eq, Hash, Debug)]
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
    #[error("Configuration validation error")]
    ValidationError,
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
}
