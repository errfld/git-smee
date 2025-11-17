use std::process::Command;

use assert_cmd::cargo;

use assert_cmd::prelude::*;
use git_smee_core::config::LifeCyclePhase;
mod common;

#[test]
fn given_git_smee_when_help_then_success() {
    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.arg("--help").assert().success();
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
