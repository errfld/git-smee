use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use git_smee_core::{
    DEFAULT_CONFIG_FILE_NAME, SmeeConfig,
    installer::{self, Error, FileSystemHookInstaller, HookInstaller, MANAGED_FILE_MARKER},
};

#[test]
fn given_standard_repo_when_installing_hooks_then_hooks_are_written_to_git_hooks_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    write_config_fixture(&repo);

    let config = read_config_from_repo(&repo);
    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();

    installer::install_hooks(&config, &installer).unwrap();

    let hooks_path = resolve_hooks_path_with_git(&repo);
    assert_eq!(
        normalize_path_for_compare(installer.effective_hooks_dir()),
        normalize_path_for_compare(&hooks_path)
    );
    assert!(hooks_path.join("pre-commit").exists());
    assert!(hooks_path.join("pre-push").exists());
}

#[test]
fn given_custom_hooks_path_when_installing_hooks_then_hooks_are_written_to_custom_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    git(&repo, &["config", "core.hooksPath", ".githooks"]);
    fs::create_dir(repo.join(".githooks")).unwrap();
    write_config_fixture(&repo);

    let config = read_config_from_repo(&repo);
    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();

    installer::install_hooks(&config, &installer).unwrap();

    let hooks_path = resolve_hooks_path_with_git(&repo);
    assert_eq!(hooks_path, repo.join(".githooks"));
    assert!(hooks_path.join("pre-commit").exists());
    assert!(hooks_path.join("pre-push").exists());
    assert!(!repo.join(".git").join("hooks").join("pre-commit").exists());
}

#[test]
fn given_worktree_when_installing_hooks_then_hooks_are_written_to_git_effective_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let main_repo = temp_dir.path().join("main");
    init_repo(&main_repo);
    fs::write(main_repo.join("README.md"), "hello").unwrap();
    git(&main_repo, &["add", "README.md"]);
    git(
        &main_repo,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "initial",
        ],
    );

    let worktree = temp_dir.path().join("wt");
    git(
        &main_repo,
        &[
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "-b",
            "worktree-branch",
        ],
    );
    write_config_fixture(&worktree);

    let config = read_config_from_repo(&worktree);
    let installer = FileSystemHookInstaller::from_path(worktree.clone()).unwrap();

    installer::install_hooks(&config, &installer).unwrap();

    let hooks_path = resolve_hooks_path_with_git(&worktree);
    assert_eq!(installer.effective_hooks_dir(), &hooks_path);
    assert!(hooks_path.join("pre-commit").exists());
    assert!(hooks_path.join("pre-push").exists());
}

#[test]
fn given_missing_custom_hooks_path_when_creating_installer_then_error_includes_resolved_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    git(&repo, &["config", "core.hooksPath", ".missing-hooks"]);

    let result = FileSystemHookInstaller::from_path(repo.clone());

    match result {
        Err(Error::HooksDirNotFound(path)) => {
            assert_eq!(
                normalize_path_for_compare(&PathBuf::from(path)),
                normalize_path_for_compare(&repo.join(".missing-hooks"))
            )
        }
        Ok(_) => panic!("expected hooks-dir-not-found error"),
        Err(error) => panic!("unexpected error: {error}"),
    }
}

#[test]
fn given_unmanaged_existing_hook_when_installing_without_force_then_error_and_file_unchanged() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    write_config_fixture(&repo);

    let hooks_path = resolve_hooks_path_with_git(&repo);
    let pre_commit = hooks_path.join("pre-commit");
    let unmanaged_content = "#!/usr/bin/env sh\necho 'custom unmanaged hook'\n";
    fs::write(&pre_commit, unmanaged_content).unwrap();

    let config = read_config_from_repo(&repo);
    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();
    let result = installer::install_hooks(&config, &installer);

    assert!(matches!(
        result,
        Err(Error::RefusingToOverwriteUnmanagedHookFile { .. })
    ));
    assert_eq!(fs::read_to_string(pre_commit).unwrap(), unmanaged_content);
}

#[test]
fn given_managed_existing_hook_when_installing_without_force_then_it_is_overwritten() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    write_config_fixture(&repo);

    let hooks_path = resolve_hooks_path_with_git(&repo);
    let pre_commit = hooks_path.join("pre-commit");
    let managed_stale_content =
        format!("#!/usr/bin/env sh\n# {MANAGED_FILE_MARKER}\necho 'stale managed hook'\n");
    fs::write(&pre_commit, managed_stale_content).unwrap();

    let config = read_config_from_repo(&repo);
    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();
    installer::install_hooks(&config, &installer).unwrap();

    let installed = fs::read_to_string(pre_commit).unwrap();
    assert!(installed.contains("git smee run pre-commit"));
}

#[test]
fn given_unmanaged_existing_hook_when_installing_with_force_then_it_is_overwritten() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);
    write_config_fixture(&repo);

    let hooks_path = resolve_hooks_path_with_git(&repo);
    let pre_commit = hooks_path.join("pre-commit");
    fs::write(
        &pre_commit,
        "#!/usr/bin/env sh\necho 'custom unmanaged hook'\n",
    )
    .unwrap();

    let config = read_config_from_repo(&repo);
    let installer = FileSystemHookInstaller::from_path_with_force(repo.clone(), true).unwrap();
    installer::install_hooks(&config, &installer).unwrap();

    let installed = fs::read_to_string(pre_commit).unwrap();
    assert!(installed.contains("git smee run pre-commit"));
}

#[test]
fn given_existing_config_when_initializing_without_force_then_refuses_overwrite() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);

    let config_path = repo.join(DEFAULT_CONFIG_FILE_NAME);
    fs::write(&config_path, "[[pre-commit]]\ncommand = \"echo custom\"\n").unwrap();

    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();
    let result = installer.install_config_file("[[pre-commit]]\ncommand = \"echo default\"\n");

    assert!(matches!(
        result,
        Err(Error::RefusingToOverwriteUnmanagedConfigFile { .. })
    ));
}

#[test]
fn given_managed_existing_config_when_initializing_without_force_then_refuses_overwrite() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);

    let config_path = repo.join(DEFAULT_CONFIG_FILE_NAME);
    fs::write(
        &config_path,
        format!("# {MANAGED_FILE_MARKER}\n\n[[pre-commit]]\ncommand = \"echo custom\"\n"),
    )
    .unwrap();

    let installer = FileSystemHookInstaller::from_path(repo.clone()).unwrap();
    let result = installer.install_config_file("[[pre-commit]]\ncommand = \"echo default\"\n");

    assert!(matches!(
        result,
        Err(Error::RefusingToOverwriteManagedConfigFile { .. })
    ));
}

#[test]
fn given_existing_config_when_initializing_with_force_then_overwrites() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    init_repo(&repo);

    let config_path = repo.join(DEFAULT_CONFIG_FILE_NAME);
    fs::write(&config_path, "[[pre-commit]]\ncommand = \"echo custom\"\n").unwrap();

    let installer = FileSystemHookInstaller::from_path_with_force(repo.clone(), true).unwrap();
    installer
        .install_config_file("[[pre-commit]]\ncommand = \"echo default\"\n")
        .unwrap();

    let updated = fs::read_to_string(config_path).unwrap();
    assert!(updated.contains("echo default"));
}

fn init_repo(repo: &Path) {
    fs::create_dir_all(repo).unwrap();
    git(repo, &["init"]);
}

fn write_config_fixture(repo: &Path) {
    let config_content = fs::read_to_string("tests/fixtures/simple_git-smee_config.toml")
        .expect("Should read fixture file");
    fs::write(repo.join(DEFAULT_CONFIG_FILE_NAME), config_content).unwrap();
}

fn read_config_from_repo(repo: &Path) -> SmeeConfig {
    let config_path = repo.join(DEFAULT_CONFIG_FILE_NAME);
    config_path
        .as_path()
        .try_into()
        .expect("Should parse test config")
}

fn resolve_hooks_path_with_git(repo: &Path) -> PathBuf {
    let output = git_output(repo, &["rev-parse", "--git-path", "hooks"]);
    let hooks = PathBuf::from(output.trim());
    if hooks.is_absolute() {
        hooks
    } else {
        repo.join(hooks)
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        repo.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        repo.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    if path.exists() {
        return fs::canonicalize(path).unwrap();
    }

    if let Some(parent) = path.parent() {
        return fs::canonicalize(parent)
            .map(|canonical_parent| canonical_parent.join(path.file_name().unwrap()))
            .unwrap_or_else(|_| path.to_path_buf());
    }

    path.to_path_buf()
}
