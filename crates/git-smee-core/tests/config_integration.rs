use std::path::PathBuf;
use std::{fs, io::Write};

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
fn given_directory_path_when_reading_then_error_is_not_a_file() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_dir = temp_dir.path().join("conf.toml");
    fs::create_dir_all(&config_dir).expect("failed to create config directory fixture");

    let result = SmeeConfig::from_toml(&config_dir);

    assert!(matches!(result, Err(config::Error::NotAFile)));
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

#[test]
fn given_unknown_hook_key_when_reading_then_actionable_parse_error() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join(".git-smee.toml");
    let mut file = fs::File::create(&config_path).expect("failed to create config fixture");
    writeln!(
        file,
        r#"
[[pre-commmit]]
command = "cargo test"
"#
    )
    .expect("failed to write config fixture");

    let result = SmeeConfig::from_toml(&config_path);

    match result {
        Err(config::Error::ParseError(error)) => {
            assert!(error.to_string().contains("pre-commmit"));
        }
        _ => panic!("expected parse error for unknown hook key"),
    }
}

#[test]
fn given_multiple_unknown_hook_keys_when_reading_then_parse_fails() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join(".git-smee.toml");
    let mut file = fs::File::create(&config_path).expect("failed to create config fixture");
    writeln!(
        file,
        r#"
[[pre-commmit]]
command = "cargo test"

[[pre-puush]]
command = "cargo fmt"
"#
    )
    .expect("failed to write config fixture");

    let result = SmeeConfig::from_toml(&config_path);

    match result {
        Err(config::Error::ParseError(error)) => {
            let message = error.to_string();
            assert!(message.contains("pre-commmit") || message.contains("pre-puush"));
        }
        _ => panic!("expected parse error for unknown hook keys"),
    }
}

#[test]
fn given_whitespace_command_when_reading_then_validation_error_with_hook_and_entry() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join(".git-smee.toml");
    let mut file = fs::File::create(&config_path).expect("failed to create config fixture");
    writeln!(
        file,
        r#"
[[pre-commit]]
command = "cargo test"

[[pre-commit]]
command = "   "
"#
    )
    .expect("failed to write config fixture");

    let result = SmeeConfig::from_toml(&config_path);

    assert!(matches!(
        result,
        Err(config::Error::ValidationError(
            config::ValidationError::EmptyCommand {
                hook_name,
                entry_index
            }
        )) if hook_name == "pre-commit" && entry_index == 2
    ));
}

#[test]
fn given_unknown_hook_definition_field_when_reading_then_parse_error_mentions_field() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join(".git-smee.toml");
    let mut file = fs::File::create(&config_path).expect("failed to create config fixture");
    writeln!(
        file,
        r#"
[[pre-commit]]
command = "cargo test"
unexpected = "value"
"#
    )
    .expect("failed to write config fixture");

    let result = SmeeConfig::from_toml(&config_path);

    match result {
        Err(config::Error::ParseError(error)) => {
            let message = error.to_string();
            assert!(message.contains("unexpected"));
        }
        _ => panic!("expected parse error for unknown hook definition fields"),
    }
}

#[test]
fn given_server_side_hooks_when_reading_then_succeed() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join(".git-smee.toml");
    let mut file = fs::File::create(&config_path).expect("failed to create config fixture");
    writeln!(
        file,
        r#"
[[pre-receive]]
command = "echo pre-receive"

[[update]]
command = "echo update"

[[proc-receive]]
command = "echo proc-receive"

[[post-receive]]
command = "echo post-receive"
"#
    )
    .expect("failed to write config fixture");

    let config = SmeeConfig::from_toml(&config_path).expect("expected config to parse");

    assert!(config.hooks.contains_key(&LifeCyclePhase::PreReceive));
    assert!(config.hooks.contains_key(&LifeCyclePhase::Update));
    assert!(config.hooks.contains_key(&LifeCyclePhase::ProcReceive));
    assert!(config.hooks.contains_key(&LifeCyclePhase::PostReceive));
}
