use std::path::PathBuf;

use git_smee_core::{
    SmeeConfig,
    config::{self, LifeCyclePhase},
};

#[test]
fn given_simple_toml_when_reading_then_succeed() {
    let path = PathBuf::from("tests/fixtures/simple_git-smee_config.toml");
    let config = SmeeConfig::from_toml(&path).expect("Should load successfully");

    let pre_commit_hooks = config
        .hooks
        .get(&LifeCyclePhase::PreCommit)
        .expect("pre-commit hooks should be present");
    assert_eq!(pre_commit_hooks.len(), 2);

    let pre_push_hooks = config
        .hooks
        .get(&LifeCyclePhase::PrePush)
        .expect("pre-push hooks should be present");
    assert_eq!(pre_push_hooks.len(), 1);
}

#[test]
fn given_invalid_path_when_reading_then_error() {
    let path = PathBuf::from("tests/fixtures/non_existent_config.toml");
    let result = SmeeConfig::from_toml(&path);
    assert!(matches!(result, Err(config::Error::MissingFile)));
}

#[test]
fn given_yaml_file_when_reading_then_error() {
    let path = PathBuf::from("tests/fixtures/wrong_extension.yaml");
    let result = SmeeConfig::from_toml(&path);
    assert!(matches!(result, Err(config::Error::NotATomlFileExtension)));
}

#[test]
fn given_missing_extension_when_reading_then_error() {
    let path = PathBuf::from("tests/fixtures/missing_extension");
    let result = SmeeConfig::from_toml(&path);
    assert!(matches!(result, Err(config::Error::CanNotReadExtension)));
}

#[test]
fn given_malformed_toml_when_reading_then_error() {
    let path = PathBuf::from("tests/fixtures/malformed_git-smee_config.toml");
    let result = SmeeConfig::from_toml(&path);
    assert!(matches!(result, Err(config::Error::ParseError(_))));
}
