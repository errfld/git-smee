use std::{fs, process::Command};

use assert_cmd::cargo;
use assert_cmd::prelude::*;
use assert_fs::TempDir;
use git_smee_core::config::LifeCyclePhase;
use git_smee_core::installer::MANAGED_FILE_MARKER;
use predicates::prelude::*;
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
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn given_non_repo_dir_when_version_then_success() {
    let non_repo_dir = TempDir::new().unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(non_repo_dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
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
            .stderr(
                predicate::str::contains("Error: Not in a git repository")
                    .and(predicate::str::contains("NotInGitRepository").not()),
            );
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
fn given_bare_repo_when_install_then_hooks_are_present() {
    let bare_repo = TempDir::new().expect("failed to create bare repo temp dir");
    git2::Repository::init_bare(bare_repo.path()).expect("failed to init bare repo");
    fs::write(
        bare_repo.path().join(".git-smee.toml"),
        r#"
[[pre-receive]]
command = "echo bare"
"#,
    )
    .expect("failed to write config");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(bare_repo.path())
        .arg("install")
        .assert()
        .success();

    assert!(bare_repo.path().join("hooks").join("pre-receive").exists());
}

#[test]
fn given_invalid_hook_when_run_then_user_friendly_error() {
    let test_repo = common::TestRepo::default();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .args(["run", "not-a-hook"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Error: Unknown lifecycle phase: not-a-hook")
                .and(predicate::str::contains("UnknownLifeCyclePhase").not()),
        );
}

#[test]
fn given_missing_config_when_install_then_user_friendly_error() {
    let test_repo = common::TestRepo::default();
    std::fs::remove_file(test_repo.config_path()).expect("Failed to remove config file");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Error: The specified configuration file is missing")
                .and(predicate::str::contains("MissingFile").not()),
        );
}

#[test]
fn given_invalid_config_when_install_then_validation_error_is_reported() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config(
        r#"
[[pre-commit]]
command = "cargo test"

[[pre-commit]]
command = "   "
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains(
                "Error: Hook 'pre-commit' entry #2: command must not be empty",
            )
            .and(predicate::str::contains("EmptyCommand").not()),
        );
}

#[test]
fn given_empty_config_when_install_then_no_hooks_present_error_is_reported() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config("");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Error: No hooks present in the configuration to install")
                .and(predicate::str::contains("NoHooksPresent").not()),
        );
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
        .stderr(
            predicate::str::contains("Error: Refusing to overwrite existing unmanaged config file")
                .and(predicate::str::contains("RefusingToOverwriteUnmanagedConfigFile").not()),
        );

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
        .stderr(
            predicate::str::contains("Error: Refusing to overwrite unmanaged hook file")
                .and(predicate::str::contains("RefusingToOverwriteUnmanagedHookFile").not()),
        );

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
    assert!(updated.contains("run pre-commit"));
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
    assert!(updated.contains("run pre-commit"));
}

#[test]
fn given_config_flag_when_installing_then_cli_uses_provided_config_path() {
    let test_repo = common::TestRepo::default();
    let custom_config = test_repo.write_config_at(
        "configs/custom.toml",
        r#"
[[pre-commit]]
command = "echo custom"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&custom_config)
        .arg("install")
        .assert()
        .success();

    assert!(test_repo.path.join(".git/hooks/pre-commit").exists());
    assert!(!test_repo.path.join(".git/hooks/pre-push").exists());
}

#[test]
fn given_git_smee_config_env_when_installing_then_cli_uses_env_config() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let env_config = test_repo.write_config_at(
        "configs/env-config.toml",
        r#"
[[pre-push]]
command = "echo env"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .env("GIT_SMEE_CONFIG", &env_config)
        .assert()
        .success();

    assert!(test_repo.path.join(".git/hooks/pre-push").exists());
    assert!(!test_repo.path.join(".git/hooks/pre-commit").exists());
}

#[cfg(unix)]
#[test]
fn given_tilde_config_flag_when_installing_then_cli_expands_home_path_for_hook_script() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let fake_home = TempDir::new().expect("failed to create fake home");
    let home_config_path = fake_home
        .path()
        .join(".config/git-smee/tilde-cli-config.toml");
    fs::create_dir_all(home_config_path.parent().expect("missing parent")).unwrap();
    fs::write(
        &home_config_path,
        r#"
[[pre-commit]]
command = "echo from-tilde-cli"
"#,
    )
    .unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .env("HOME", fake_home.path())
        .arg("--config")
        .arg("~/.config/git-smee/tilde-cli-config.toml")
        .arg("install")
        .assert()
        .success();

    let hook_content =
        fs::read_to_string(test_repo.path.join(".git/hooks/pre-commit")).expect("missing hook");

    assert!(hook_content.contains(home_config_path.to_string_lossy().as_ref()));
    assert!(!hook_content.contains("~/.config/git-smee/tilde-cli-config.toml"));
}

#[cfg(unix)]
#[test]
fn given_tilde_git_smee_config_env_when_installing_then_cli_expands_home_path_for_hook_script() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let fake_home = TempDir::new().expect("failed to create fake home");
    let home_config_path = fake_home
        .path()
        .join(".config/git-smee/tilde-env-config.toml");
    fs::create_dir_all(home_config_path.parent().expect("missing parent")).unwrap();
    fs::write(
        &home_config_path,
        r#"
[[pre-push]]
command = "echo from-tilde-env"
"#,
    )
    .unwrap();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .env("HOME", fake_home.path())
        .env(
            "GIT_SMEE_CONFIG",
            "~/.config/git-smee/tilde-env-config.toml",
        )
        .arg("install")
        .assert()
        .success();

    let hook_content =
        fs::read_to_string(test_repo.path.join(".git/hooks/pre-push")).expect("missing hook");

    assert!(hook_content.contains(home_config_path.to_string_lossy().as_ref()));
    assert!(!hook_content.contains("~/.config/git-smee/tilde-env-config.toml"));
}

#[test]
fn given_config_flag_and_env_when_installing_then_flag_takes_precedence() {
    let test_repo = common::TestRepo::default();
    let env_config = test_repo.write_config_at(
        "configs/env-invalid.toml",
        r#"
[[pre-commit]]
command = "   "
"#,
    );
    let cli_config = test_repo.write_config_at(
        "configs/cli-config.toml",
        r#"
[[pre-push]]
command = "echo cli"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&cli_config)
        .arg("install")
        .env("GIT_SMEE_CONFIG", &env_config)
        .assert()
        .success();

    assert!(test_repo.path.join(".git/hooks/pre-push").exists());
    assert!(!test_repo.path.join(".git/hooks/pre-commit").exists());
}

#[test]
fn given_config_flag_when_running_hook_then_run_uses_provided_config_path() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let custom_config = test_repo.write_config_at(
        "configs/run-custom.toml",
        r#"
[[pre-commit]]
command = "echo from-custom-config"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&custom_config)
        .arg("run")
        .arg("pre-commit")
        .assert()
        .success();
}

#[test]
fn given_git_smee_config_env_when_running_hook_then_run_uses_env_config() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let env_config = test_repo.write_config_at(
        "configs/run-env.toml",
        r#"
[[pre-commit]]
command = "echo from-env-config"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("run")
        .arg("pre-commit")
        .env("GIT_SMEE_CONFIG", &env_config)
        .assert()
        .success();
}

#[test]
fn given_failing_hook_when_running_then_cli_surfaces_non_zero_exit_code() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config(
        r#"
[[pre-commit]]
command = "exit 1"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("run")
        .arg("pre-commit")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Error: Hook execution failed with exit code")
                .and(predicate::str::contains("ExecutionFailed").not()),
        );
}

#[test]
fn given_bare_repo_when_running_then_hook_executes() {
    let bare_repo = TempDir::new().expect("failed to create bare repo temp dir");
    git2::Repository::init_bare(bare_repo.path()).expect("failed to init bare repo");
    fs::write(
        bare_repo.path().join(".git-smee.toml"),
        r#"
[[pre-receive]]
command = "echo bare-run"
"#,
    )
    .expect("failed to write config");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(bare_repo.path())
        .args(["run", "pre-receive"])
        .assert()
        .success();
}

#[test]
fn given_custom_config_path_when_installing_then_hook_script_contains_executable_and_config_path() {
    let test_repo = common::TestRepo::default();
    let custom_config = test_repo.write_config_at(
        "configs/hook-config.toml",
        r#"
[[pre-commit]]
command = "echo custom"
"#,
    );
    let expected_executable = cargo::cargo_bin!("git-smee");
    let expected_config_path = test_repo.path.join("configs/hook-config.toml");

    let mut cmd = Command::new(expected_executable);
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&custom_config)
        .arg("install")
        .assert()
        .success();

    let hook_content =
        fs::read_to_string(test_repo.path.join(".git/hooks/pre-commit")).expect("missing hook");

    assert!(hook_content.contains("--config"));
    assert!(hook_content.contains(expected_executable.to_string_lossy().as_ref()));
    assert!(hook_content.contains(expected_config_path.to_string_lossy().as_ref()));
}

#[test]
fn given_default_config_when_installing_then_hook_script_keeps_portable_relative_path() {
    let test_repo = common::TestRepo::default();

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    let hook_content =
        fs::read_to_string(test_repo.path.join(".git/hooks/pre-commit")).expect("missing hook");

    #[cfg(unix)]
    assert!(hook_content.contains("GIT_SMEE_CONFIG='.git-smee.toml'"));

    #[cfg(windows)]
    assert!(hook_content.contains("set \"GIT_SMEE_CONFIG=.git-smee.toml\""));
}

#[test]
fn given_special_character_config_path_when_installing_then_hook_script_escapes_path() {
    let test_repo = common::TestRepo::default();
    let custom_config = test_repo.write_config_at(
        "configs/it's 100% ready/hook config.toml",
        r#"
[[pre-commit]]
command = "echo custom"
"#,
    );

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&custom_config)
        .arg("install")
        .assert()
        .success();

    let hook_content =
        fs::read_to_string(test_repo.path.join(".git/hooks/pre-commit")).expect("missing hook");

    #[cfg(unix)]
    {
        let expected = custom_config.to_string_lossy().replace('\'', "'\"'\"'");
        assert!(hook_content.contains(&expected));
    }

    #[cfg(windows)]
    {
        let expected = custom_config
            .to_string_lossy()
            .replace('"', "\"\"")
            .replace('%', "%%");
        assert!(hook_content.contains(&expected));
    }
}

#[cfg(unix)]
#[test]
fn given_minimal_path_when_running_installed_hook_then_hook_uses_absolute_git_smee_path() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config(
        r#"
[[pre-commit]]
command = "true"
"#,
    );

    let mut install = Command::new(cargo::cargo_bin!("git-smee"));
    install
        .current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    let hook_path = test_repo.path.join(".git/hooks/pre-commit");
    let mut hook = Command::new(hook_path);
    hook.current_dir(&test_repo.path)
        .env("PATH", "/usr/bin:/bin")
        .assert()
        .success();
}

#[test]
fn given_custom_config_path_when_initializing_then_init_writes_requested_file() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove default config");
    let custom_config_path = test_repo.path.join("configs/init-config.toml");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(&test_repo.path)
        .arg("--config")
        .arg(&custom_config_path)
        .arg("init")
        .assert()
        .success();

    assert!(custom_config_path.exists());
    assert!(!test_repo.config_path().exists());
}

#[test]
fn given_bare_repo_when_initializing_then_init_writes_default_config() {
    let bare_repo = TempDir::new().expect("failed to create bare repo temp dir");
    git2::Repository::init_bare(bare_repo.path()).expect("failed to init bare repo");

    let mut cmd = Command::new(cargo::cargo_bin!("git-smee"));
    cmd.current_dir(bare_repo.path())
        .arg("init")
        .assert()
        .success();

    assert!(bare_repo.path().join(".git-smee.toml").exists());
}
