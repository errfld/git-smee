use std::fs;

use assert_cmd::{Command, cargo};
use git_smee_core::installer::MANAGED_FILE_MARKER;
use predicates::prelude::*;

mod common;

#[test]
fn given_healthy_repo_when_doctor_then_successful_sections_are_reported() {
    let test_repo = common::TestRepo::default();
    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("git-smee doctor: Ok")
                .and(predicate::str::contains("ok:"))
                .and(predicate::str::contains("warnings:\n  - none"))
                .and(predicate::str::contains("errors:\n  - none"))
                .and(predicate::str::contains(
                    "managed wrapper is installed for pre-commit",
                )),
        );
}

#[test]
fn given_healthy_repo_when_doctor_json_then_stable_json_is_reported() {
    let test_repo = common::TestRepo::default();
    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(r#""status": "ok""#)
                .and(predicate::str::contains(r#""repository_root""#))
                .and(predicate::str::contains(r#""hooks_dir""#))
                .and(predicate::str::contains(r#""errors": []"#)),
        );
}

#[test]
fn given_installed_hooks_when_status_then_reports_coverage() {
    let test_repo = common::TestRepo::default();
    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("install")
        .assert()
        .success();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("status")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("git-smee status: Ok")
                .and(predicate::str::contains(
                    "pre-commit: configured commands=1, installed",
                ))
                .and(predicate::str::contains(
                    "pre-push: configured commands=1, installed",
                ))
                .and(predicate::str::contains("next actions:\n  - none")),
        );
}

#[test]
fn given_missing_and_unmanaged_hooks_when_status_then_reports_next_actions() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git/hooks/pre-commit");
    fs::write(&pre_commit, "#!/usr/bin/env sh\necho unmanaged\n").unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("status")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("git-smee status: Drift")
                .and(predicate::str::contains(
                    "pre-commit: configured commands=1, unmanaged",
                ))
                .and(predicate::str::contains(
                    "pre-push: configured commands=1, missing",
                ))
                .and(predicate::str::contains("run git smee install --force"))
                .and(predicate::str::contains("run git smee install")),
        );
}

#[test]
fn given_stale_and_obsolete_managed_hooks_when_status_then_reports_drift_without_modifying() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git/hooks/pre-commit");
    fs::write(
        &pre_commit,
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\n/old/git-smee --config old.toml run pre-commit\n"),
    )
    .unwrap();
    let obsolete = test_repo.path.join(".git/hooks/commit-msg");
    fs::write(
        &obsolete,
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\n/old/git-smee run commit-msg\n"),
    )
    .unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("status")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pre-commit: configured commands=1, stale")
                .and(predicate::str::contains(
                    "commit-msg: obsolete managed wrapper",
                ))
                .and(predicate::str::contains(
                    "remove obsolete managed hook .git/hooks/commit-msg",
                )),
        );

    assert!(
        obsolete.exists(),
        "status must not remove obsolete managed hooks"
    );
}

#[test]
fn given_marker_only_in_body_when_status_then_treats_hook_as_unmanaged() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config(
        r#"
        [[commit-msg]]
        command = "echo commit message"
        "#,
    );
    let commit_msg = test_repo.path.join(".git/hooks/commit-msg");
    fs::write(
        &commit_msg,
        format!("#!/usr/bin/env sh\necho '{MANAGED_FILE_MARKER}'\n"),
    )
    .unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("status")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("commit-msg: configured commands=1, unmanaged")
                .and(predicate::str::contains("obsolete managed wrapper").not())
                .and(predicate::str::contains("run git smee install --force")),
        );
}

#[test]
fn given_unconfigured_marker_only_in_body_when_status_then_does_not_recommend_removal() {
    let test_repo = common::TestRepo::default();
    let commit_msg = test_repo.path.join(".git/hooks/commit-msg");
    fs::write(
        &commit_msg,
        format!("#!/usr/bin/env sh\necho '{MANAGED_FILE_MARKER}'\n"),
    )
    .unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("obsolete managed wrapper").not());
}

#[test]
fn given_drift_when_status_json_then_stable_json_is_reported() {
    let test_repo = common::TestRepo::default();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(r#""status": "drift""#)
                .and(predicate::str::contains(r#""phase": "pre-commit""#))
                .and(predicate::str::contains(r#""configured_command_count": 1"#))
                .and(predicate::str::contains(r#""state": "missing""#))
                .and(predicate::str::contains(r#""next_actions""#)),
        );
}

#[test]
fn given_missing_config_when_doctor_then_actionable_error_is_reported() {
    let test_repo = common::TestRepo::default();
    fs::remove_file(test_repo.config_path()).expect("failed to remove config");

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("errors:")
                .and(predicate::str::contains("config problem"))
                .and(predicate::str::contains("run git smee init")),
        )
        .stderr(predicate::str::contains(
            "doctor found repository setup errors",
        ));
}

#[test]
fn given_malformed_config_when_doctor_then_parse_error_is_reported() {
    let test_repo = common::TestRepo::default();
    test_repo.write_config("[[pre-commit]]\ncommand = ");

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("config problem")
                .and(predicate::str::contains("fix the TOML file")),
        );
}

#[test]
fn given_unmanaged_hook_when_doctor_then_collision_is_reported() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git/hooks/pre-commit");
    fs::write(&pre_commit, "#!/usr/bin/env sh\necho unmanaged\n").unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("unmanaged hook file blocks install for pre-commit")
                .and(predicate::str::contains("git smee install --force")),
        );
}

#[test]
fn given_marker_only_in_body_when_doctor_then_hook_is_unmanaged() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git/hooks/pre-commit");
    fs::write(
        &pre_commit,
        format!("#!/usr/bin/env sh\necho '{MANAGED_FILE_MARKER}'\n"),
    )
    .unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("unmanaged hook file blocks install for pre-commit")
                .and(predicate::str::contains("managed wrapper is installed for pre-commit").not()),
        );
}

#[test]
fn given_custom_hooks_path_when_doctor_then_effective_path_is_reported() {
    let test_repo = common::TestRepo::default();
    std::process::Command::new("git")
        .current_dir(&test_repo.path)
        .args(["config", "core.hooksPath", ".githooks"])
        .status()
        .expect("failed to configure hooksPath");

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("hooks directory:")
                .and(predicate::str::contains(".githooks"))
                .and(predicate::str::contains("run git smee install")),
        );
}

#[test]
fn given_stale_managed_hook_when_doctor_then_reinstall_warning_is_reported() {
    let test_repo = common::TestRepo::default();
    let pre_commit = test_repo.path.join(".git/hooks/pre-commit");
    fs::write(
        &pre_commit,
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\n/old/git-smee --config old.toml run pre-commit\n"),
    )
    .unwrap();
    let pre_push = test_repo.path.join(".git/hooks/pre-push");
    fs::write(
        &pre_push,
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\n/old/git-smee --config old.toml run pre-push\n"),
    )
    .unwrap();

    Command::new(cargo::cargo_bin!("git-smee"))
        .current_dir(&test_repo.path)
        .arg("doctor")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("git-smee doctor: Warning")
                .and(predicate::str::contains(
                    "stale managed wrapper for pre-commit",
                ))
                .and(predicate::str::contains("run git smee install"))
                .and(predicate::str::contains("errors:\n  - none")),
        );
}
