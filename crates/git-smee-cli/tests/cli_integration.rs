use std::{fs, process::Command};

use assert_cmd::cargo;
use assert_cmd::prelude::*;
use assert_fs::TempDir;
use git_smee_core::config::LifeCyclePhase;
use git_smee_core::installer::MANAGED_FILE_MARKER;
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

#[test]
fn given_existing_config_when_init_without_force_then_it_refuses_to_overwrite() {
    let test_repo = common::TestRepo::default();
    let original = fs::read_to_string(test_repo.config_path()).unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("init")
        .assert()
        .failure()
        .stderr(contains("RefusingToOverwriteUnmanagedConfigFile"));

    let after = fs::read_to_string(test_repo.config_path()).unwrap();
    assert_eq!(after, original);
}

#[test]
fn given_existing_config_when_init_with_force_then_it_overwrites_config() {
    let test_repo = common::TestRepo::default();
    fs::write(
        test_repo.config_path(),
        "[[pre-commit]]\ncommand = \"echo custom config\"\n",
    )
    .unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("init")
        .arg("--force")
        .assert()
        .success();

    let initialized = fs::read_to_string(test_repo.config_path()).unwrap();
    assert!(initialized.contains(MANAGED_FILE_MARKER));
    assert!(initialized.contains("Default pre-commit hook"));
}

#[test]
fn given_unmanaged_hook_when_install_without_force_then_it_fails_and_preserves_hook() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git").join("hooks").join("pre-commit");
    let unmanaged = "#!/usr/bin/env sh\necho 'custom unmanaged hook'\n";
    fs::write(&pre_commit, unmanaged).unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .failure()
        .stderr(contains("RefusingToOverwriteUnmanagedHookFile"));

    let after = fs::read_to_string(pre_commit).unwrap();
    assert_eq!(after, unmanaged);
}

#[test]
fn given_managed_hook_when_install_without_force_then_it_overwrites_managed_hook() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git").join("hooks").join("pre-commit");
    let stale_managed =
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\necho 'stale managed hook'\n");
    fs::write(&pre_commit, stale_managed).unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    let updated = fs::read_to_string(pre_commit).unwrap();
    assert!(updated.contains("git smee run pre-commit"));
}

#[test]
fn given_unmanaged_hook_when_install_with_force_then_it_overwrites_hook() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git").join("hooks").join("pre-commit");
    fs::write(
        &pre_commit,
        "#!/usr/bin/env sh\necho 'custom unmanaged hook'\n",
    )
    .unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .arg("--force")
        .assert()
        .success();

    let updated = fs::read_to_string(pre_commit).unwrap();
    assert!(updated.contains("git smee run pre-commit"));
}
