use std::process::Command;

use assert_cmd::cargo;
use assert_cmd::prelude::*;
use assert_fs::TempDir;
use git_smee_core::config::LifeCyclePhase;
use predicates::str::contains;
mod common;

#[test]
fn given_git_smee_when_help_then_success() {
    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.arg("--help").assert().success();
}

#[test]
fn given_non_repo_dir_when_help_then_success() {
    let non_repo_dir = TempDir::new().unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(non_repo_dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage:"));
}

#[test]
fn given_non_repo_dir_when_version_then_success() {
    let non_repo_dir = TempDir::new().unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(non_repo_dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn given_non_repo_dir_when_repo_commands_then_not_in_git_repository_error() {
    let non_repo_dir = TempDir::new().unwrap();

    for args in [
        ["install"].as_slice(),
        ["init"].as_slice(),
        ["run", "pre-commit"].as_slice(),
    ] {
        let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
        cmd.current_dir(non_repo_dir.path())
            .args(args)
            .assert()
            .failure()
            .stderr(contains("NotInGitRepository"));
    }
}

#[test]
fn given_git_smee_when_install_then_hooks_are_present() {
    let test_repo = common::TestRepo::default();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();
    test_repo.assert_hooks_installed(vec![LifeCyclePhase::PreCommit, LifeCyclePhase::PrePush]);
}
