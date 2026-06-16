use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Not in a git repository")]
    NotInGitRepository,
    #[error("Failed to change directory: {0}")]
    FailedToChangeDirectory(#[from] std::io::Error),
    #[error("Failed to execute git: {0}")]
    FailedToExecuteGit(std::io::Error),
    #[error("Could not resolve git path '{git_path}' from '{repository_root}': {stderr}")]
    FailedToResolveGitPath {
        repository_root: String,
        git_path: String,
        stderr: String,
    },
    #[error("Git returned an empty path for '{git_path}' in repository '{repository_root}'")]
    EmptyGitPath {
        repository_root: String,
        git_path: String,
    },
    #[error("git rev-parse {flag} failed: {stderr}")]
    FailedToQueryGitRevParse { flag: String, stderr: String },
    #[error("git rev-parse {flag} returned non-UTF-8 output on non-Unix platforms")]
    InvalidGitPathEncoding { flag: String },
}

/// Finds the git repository root.
///
/// - For a non-bare repository, this resolves the top-level worktree path.
/// - For a bare repository, this resolves to the current working directory.
///
/// # Examples
///
/// ```rust
/// use git_smee_core::find_git_root;
/// use std::{env, process::Command};
/// use tempfile::tempdir;
///
/// let temp_dir = tempdir().unwrap();
/// Command::new("git")
///     .arg("init")
///     .current_dir(temp_dir.path())
///     .output()
///     .unwrap();
/// let nested = temp_dir.path().join("nested");
/// std::fs::create_dir(&nested).unwrap();
///
/// let original_dir = env::current_dir().unwrap();
/// env::set_current_dir(&nested).unwrap();
///
/// let repo_root = find_git_root().unwrap();
///
/// env::set_current_dir(&original_dir).unwrap();
///
/// let normalize = |path: &std::path::Path| {
///     let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
///     canonical
///         .to_string_lossy()
///         .replace('\\', "/")
///         .trim_start_matches("//?/")
///         .to_string()
/// };
/// assert_eq!(normalize(&repo_root), normalize(temp_dir.path()));
/// ```
pub fn find_git_root() -> Result<PathBuf, Error> {
    let current_dir = env::current_dir().map_err(Error::FailedToChangeDirectory)?;
    find_git_root_from_path(&current_dir)
}

fn find_git_root_from_path(current_dir: &Path) -> Result<PathBuf, Error> {
    if git_rev_parse_bool(current_dir, "--is-inside-work-tree")?
        && let Some(root) = git_rev_parse_path(current_dir, "--show-toplevel")?
    {
        let canonical_root = root
            .canonicalize()
            .map_err(Error::FailedToChangeDirectory)?;
        if !git_rev_parse_bool(current_dir, "--is-bare-repository")?
            && canonical_root.file_name() == Some(OsStr::new(".git"))
            && let Some(worktree_root) = canonical_root.parent()
        {
            return worktree_root
                .canonicalize()
                .map_err(Error::FailedToChangeDirectory);
        }
        return Ok(canonical_root);
    }

    if git_rev_parse_bool(current_dir, "--is-inside-git-dir")?
        && let Some(git_dir) = git_rev_parse_path(current_dir, "--absolute-git-dir")?
    {
        if git_rev_parse_bool(current_dir, "--is-bare-repository")? {
            return git_dir
                .canonicalize()
                .map_err(Error::FailedToChangeDirectory);
        }

        if git_dir.file_name() == Some(OsStr::new(".git"))
            && let Some(worktree_root) = git_dir.parent()
        {
            return worktree_root
                .canonicalize()
                .map_err(Error::FailedToChangeDirectory);
        }

        return git_dir
            .canonicalize()
            .map_err(Error::FailedToChangeDirectory);
    }

    if git_rev_parse_bool(current_dir, "--is-bare-repository")? {
        if let Some(git_dir) = git_rev_parse_path(current_dir, "--absolute-git-dir")? {
            return git_dir
                .canonicalize()
                .map_err(Error::FailedToChangeDirectory);
        }
        return current_dir
            .canonicalize()
            .map_err(Error::FailedToChangeDirectory);
    }

    Err(Error::NotInGitRepository)
}

fn git_rev_parse_bool(current_dir: &Path, flag: &str) -> Result<bool, Error> {
    let output = Command::new("git")
        .current_dir(current_dir)
        .arg("rev-parse")
        .arg(flag)
        .output()
        .map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        if should_treat_rev_parse_failure_as_not_in_repository(current_dir, output.status.code()) {
            return Ok(false);
        }
        let stderr = stderr_or_status(&output.stderr, output.status.code());
        return Err(Error::FailedToQueryGitRevParse {
            flag: flag.to_string(),
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn git_rev_parse_path(current_dir: &Path, flag: &str) -> Result<Option<PathBuf>, Error> {
    let output = Command::new("git")
        .current_dir(current_dir)
        .arg("rev-parse")
        .arg(flag)
        .output()
        .map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        if should_treat_rev_parse_failure_as_not_in_repository(current_dir, output.status.code()) {
            return Ok(None);
        }
        let stderr = stderr_or_status(&output.stderr, output.status.code());
        return Err(Error::FailedToQueryGitRevParse {
            flag: flag.to_string(),
            stderr,
        });
    }

    let trimmed = trim_git_output_path(&output.stdout);
    if trimmed.is_empty() {
        return Ok(None);
    }

    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let value = OsString::from_vec(trimmed.to_vec());
        Ok(Some(PathBuf::from(value)))
    }

    #[cfg(not(unix))]
    {
        let value =
            String::from_utf8(trimmed.to_vec()).map_err(|_| Error::InvalidGitPathEncoding {
                flag: flag.to_string(),
            })?;
        Ok(Some(PathBuf::from(value)))
    }
}

fn should_treat_rev_parse_failure_as_not_in_repository(
    current_dir: &Path,
    status_code: Option<i32>,
) -> bool {
    status_code == Some(128) && !has_git_repository_context(current_dir)
}

fn has_git_repository_context(current_dir: &Path) -> bool {
    if env::var_os("GIT_DIR").is_some() || env::var_os("GIT_WORK_TREE").is_some() {
        return true;
    }

    current_dir.ancestors().any(|ancestor| {
        ancestor.join(".git").exists()
            || (ancestor.join("HEAD").is_file()
                && ancestor.join("objects").is_dir()
                && ancestor.join("refs").is_dir())
    })
}

fn stderr_or_status(stderr: &[u8], status_code: Option<i32>) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if stderr.is_empty() {
        match status_code {
            Some(code) => format!("git exited with status {code}"),
            None => "git terminated by signal".to_string(),
        }
    } else {
        stderr
    }
}

fn trim_git_output_path(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

/// Validates that we're in a git repository and changes to the repository root.
///
/// # Examples
///
/// ```rust
/// use git_smee_core::ensure_in_repo_root;
/// use std::{env, process::Command};
/// use tempfile::tempdir;
///
/// let temp_dir = tempdir().unwrap();
/// Command::new("git")
///     .arg("init")
///     .current_dir(temp_dir.path())
///     .output()
///     .unwrap();
/// let nested = temp_dir.path().join("nested");
/// std::fs::create_dir(&nested).unwrap();
///
/// let original_dir = env::current_dir().unwrap();
/// env::set_current_dir(&nested).unwrap();
///
/// ensure_in_repo_root().unwrap();
/// let current_dir = env::current_dir().unwrap();
///
/// env::set_current_dir(&original_dir).unwrap();
///
/// let normalize = |path: &std::path::Path| {
///     let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
///     canonical
///         .to_string_lossy()
///         .replace('\\', "/")
///         .trim_start_matches("//?/")
///         .to_string()
/// };
/// assert_eq!(normalize(&current_dir), normalize(temp_dir.path()));
/// ```
pub fn ensure_in_repo_root() -> Result<(), Error> {
    let git_root = find_git_root()?;
    env::set_current_dir(&git_root).map_err(Error::FailedToChangeDirectory)
}

/// Resolves a Git path (as interpreted by `git rev-parse --git-path`) from the
/// given repository root.
pub fn resolve_git_path(repository_root: &Path, git_path: &str) -> Result<PathBuf, Error> {
    let output = git_command_with_explicit_repo(repository_root)
        .arg("rev-parse")
        .arg("--git-path")
        .arg(git_path)
        .output()
        .map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::FailedToResolveGitPath {
            repository_root: repository_root.display().to_string(),
            git_path: git_path.to_string(),
            stderr: if stderr.is_empty() {
                format!("git exited with status {}", output.status)
            } else {
                stderr
            },
        });
    }

    let raw_path = trim_git_output_path(&output.stdout);
    if raw_path.is_empty() {
        return Err(Error::EmptyGitPath {
            repository_root: repository_root.display().to_string(),
            git_path: git_path.to_string(),
        });
    }

    let path = git_output_path_to_path_buf(raw_path, git_path)?;
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(repository_root.join(path))
    }
}

#[cfg_attr(unix, allow(unused_variables))]
fn git_output_path_to_path_buf(bytes: &[u8], flag: &str) -> Result<PathBuf, Error> {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }

    #[cfg(not(unix))]
    {
        let value =
            String::from_utf8(bytes.to_vec()).map_err(|_| Error::InvalidGitPathEncoding {
                flag: flag.to_string(),
            })?;
        Ok(PathBuf::from(value))
    }
}

fn git_command_with_explicit_repo(repository_root: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(repository_root);
    for env_name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(env_name);
    }
    command
}

/// Resolves the effective hooks directory used by Git for the repository.
pub fn resolve_hooks_path(repository_root: &Path) -> Result<PathBuf, Error> {
    resolve_git_path(repository_root, "hooks")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::process_state_lock;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn given_current_dir_is_git_root_when_finding_root_then_returns_current_dir() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        let result_path = result.unwrap();
        assert_eq!(
            normalize_path_for_compare(&result_path),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_current_dir_is_subdirectory_of_repo_when_finding_root_then_returns_repo_root() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&sub_dir).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        let result_path = result.unwrap();
        assert_eq!(
            normalize_path_for_compare(&result_path),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_current_dir_is_deeply_nested_subdirectory_when_finding_root_then_returns_repo_root() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);

        // Create nested directories: a/b/c
        let nested_path = temp_dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested_path).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&nested_path).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        let result_path = result.unwrap();
        assert_eq!(
            normalize_path_for_compare(&result_path),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_not_in_git_repo_when_finding_root_then_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let nested = temp_dir.path().join("not-a-repo").join("deep");
        fs::create_dir_all(&nested).unwrap();

        let result = find_git_root_from_path(&nested);

        assert!(matches!(result, Err(Error::NotInGitRepository)));
    }

    #[test]
    fn given_git_reports_not_repo_in_non_english_when_finding_root_then_returns_not_in_repo() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        let git_bin_dir = temp_dir.path().join("bin");
        fs::create_dir(&git_bin_dir).unwrap();
        write_fake_git_that_reports_not_repo_in_non_english(&git_bin_dir);

        let original_path = env::var_os("PATH");
        let fake_path = prepend_to_path(&git_bin_dir, original_path.as_ref());
        unsafe { env::set_var("PATH", fake_path) };

        let result = find_git_root_from_path(temp_dir.path());

        match original_path {
            Some(value) => unsafe { env::set_var("PATH", value) },
            None => unsafe { env::remove_var("PATH") },
        }

        assert!(matches!(result, Err(Error::NotInGitRepository)));
    }

    #[test]
    fn given_git_repo_has_malformed_config_when_finding_root_then_surfaces_rev_parse_error() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        fs::write(temp_dir.path().join(".git").join("config"), "[broken\n").unwrap();

        let result = find_git_root_from_path(temp_dir.path());

        assert!(matches!(
            result,
            Err(Error::FailedToQueryGitRevParse { .. })
        ));
    }

    #[test]
    fn given_bare_repo_when_finding_root_then_returns_current_dir() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init", "--bare"]);

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&result.unwrap()),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_in_git_repo_when_ensuring_in_repo_root_then_succeeds() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&sub_dir).unwrap();

        let result = ensure_in_repo_root();
        // Check current directory before resetting
        let current = env::current_dir().unwrap();
        let git_exists = current.join(".git").exists();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&current),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
        assert!(git_exists);
    }

    #[test]
    fn given_not_in_git_repo_when_ensuring_in_repo_root_then_returns_error() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        // Deliberately don't create .git directory

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = ensure_in_repo_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_err());
        assert!(matches!(result, Err(Error::NotInGitRepository)));
    }

    #[test]
    fn given_bare_repo_when_ensuring_in_repo_root_then_succeeds() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init", "--bare"]);
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = ensure_in_repo_root();
        let current = env::current_dir().unwrap();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&current),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_standard_repo_when_resolving_hooks_path_then_returns_dot_git_hooks() {
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);

        let hooks_path = resolve_hooks_path(temp_dir.path()).unwrap();

        assert_eq!(hooks_path, temp_dir.path().join(".git").join("hooks"));
    }

    #[test]
    fn given_custom_core_hooks_path_when_resolving_hooks_path_then_returns_custom_path() {
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        git(temp_dir.path(), &["config", "core.hooksPath", ".githooks"]);

        let hooks_path = resolve_hooks_path(temp_dir.path()).unwrap();

        assert_eq!(hooks_path, temp_dir.path().join(".githooks"));
    }

    #[test]
    fn given_worktree_when_resolving_hooks_path_then_matches_git_output() {
        let temp_dir = TempDir::new().unwrap();
        let main_repo = temp_dir.path().join("main");
        fs::create_dir(&main_repo).unwrap();
        git(&main_repo, &["init"]);
        fs::write(main_repo.join("README.md"), "test").unwrap();
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
                "init",
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
                "wt-branch",
            ],
        );

        let resolved = resolve_hooks_path(&worktree).unwrap();
        let expected = git_output(&worktree, &["rev-parse", "--git-path", "hooks"]);
        let expected_path = PathBuf::from(expected.trim());

        assert_eq!(resolved, expected_path);
    }

    #[test]
    fn given_current_dir_is_git_directory_when_finding_root_then_returns_worktree_root() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        let git_dir = temp_dir.path().join(".git");
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&git_dir).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&result.unwrap()),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_current_dir_is_git_directory_when_ensuring_in_repo_root_then_changes_to_worktree_root()
    {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        let git_dir = temp_dir.path().join(".git");
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&git_dir).unwrap();

        let result = ensure_in_repo_root();
        let current = env::current_dir().unwrap();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&current),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_git_dir_env_in_hook_context_when_finding_root_then_returns_worktree_root() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        let git_dir = temp_dir.path().join(".git");
        let original_dir = env::current_dir().unwrap();
        let original_git_dir = env::var_os("GIT_DIR");
        env::set_current_dir(&git_dir).unwrap();

        // Git hook processes in non-bare repositories typically inherit GIT_DIR=.
        unsafe { env::set_var("GIT_DIR", ".") };

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();
        match original_git_dir {
            Some(value) => unsafe { env::set_var("GIT_DIR", value) },
            None => unsafe { env::remove_var("GIT_DIR") },
        }

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&result.unwrap()),
            normalize_path_for_compare(&temp_dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn given_bare_git_dir_env_outside_repo_when_finding_root_then_returns_bare_repo_path() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        let bare_repo = temp_dir.path().join("remote.git");
        fs::create_dir(&bare_repo).unwrap();
        git(&bare_repo, &["init", "--bare"]);

        let outside_dir = temp_dir.path().join("outside");
        fs::create_dir(&outside_dir).unwrap();
        let original_dir = env::current_dir().unwrap();
        let original_git_dir = env::var_os("GIT_DIR");
        let original_git_work_tree = env::var_os("GIT_WORK_TREE");
        env::set_current_dir(&outside_dir).unwrap();
        unsafe { env::set_var("GIT_DIR", bare_repo.as_os_str()) };
        unsafe { env::remove_var("GIT_WORK_TREE") };

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();
        match original_git_dir {
            Some(value) => unsafe { env::set_var("GIT_DIR", value) },
            None => unsafe { env::remove_var("GIT_DIR") },
        }
        match original_git_work_tree {
            Some(value) => unsafe { env::set_var("GIT_WORK_TREE", value) },
            None => unsafe { env::remove_var("GIT_WORK_TREE") },
        }

        assert!(result.is_ok());
        assert_eq!(
            normalize_path_for_compare(&result.unwrap()),
            normalize_path_for_compare(&bare_repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn given_git_output_with_trailing_newline_when_trimming_then_only_newline_is_removed() {
        assert_eq!(trim_git_output_path(b"/repo/path\n"), b"/repo/path");
    }

    #[test]
    fn given_git_output_with_trailing_space_when_trimming_then_space_is_preserved() {
        assert_eq!(trim_git_output_path(b"/repo/path \n"), b"/repo/path ");
    }

    #[test]
    fn given_git_path_output_with_trailing_space_when_resolving_then_space_is_preserved() {
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);
        git(temp_dir.path(), &["config", "core.hooksPath", ".githooks "]);

        let resolved = resolve_git_path(temp_dir.path(), "hooks").unwrap();

        assert_eq!(
            normalize_path_for_compare(&resolved),
            normalize_path_for_compare(&temp_dir.path().join(".githooks "))
        );
    }

    #[cfg(unix)]
    #[test]
    fn given_non_utf8_git_path_output_when_decoding_then_bytes_are_preserved() {
        use std::os::unix::ffi::OsStrExt;

        let path = git_output_path_to_path_buf(b".git/hooks-\xFF", "hooks").unwrap();

        assert_eq!(path.as_os_str().as_bytes(), b".git/hooks-\xFF");
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = git_command_with_explicit_repo(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(repo: &Path, args: &[&str]) -> String {
        let output = git_command_with_explicit_repo(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn normalize_path_for_compare(path: &Path) -> String {
        let normalized = path.to_string_lossy().replace('\\', "/");
        normalized
            .strip_prefix("//?/")
            .unwrap_or(&normalized)
            .to_string()
    }

    fn prepend_to_path(
        new_entry: &Path,
        original_path: Option<&std::ffi::OsString>,
    ) -> std::ffi::OsString {
        let mut entries = vec![new_entry.to_path_buf()];
        if let Some(original_path) = original_path {
            entries.extend(env::split_paths(original_path));
        }
        env::join_paths(entries).unwrap()
    }

    #[cfg(unix)]
    fn write_fake_git_that_reports_not_repo_in_non_english(bin_dir: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let git_path = bin_dir.join("git");
        fs::write(
            &git_path,
            "#!/bin/sh\necho 'fatal: no es un repositorio git' >&2\nexit 128\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&git_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&git_path, permissions).unwrap();
    }

    #[cfg(windows)]
    fn write_fake_git_that_reports_not_repo_in_non_english(bin_dir: &Path) {
        fs::write(
            bin_dir.join("git.cmd"),
            "@echo off\r\necho fatal: no es un repositorio git 1>&2\r\nexit /b 128\r\n",
        )
        .unwrap();
    }

    #[test]
    fn given_explicit_repo_git_helper_when_git_dir_env_is_contaminated_then_it_ignores_it() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        let bare_repo = temp_dir.path().join("remote.git");
        fs::create_dir(&bare_repo).unwrap();
        git(&bare_repo, &["init", "--bare"]);

        let repo = temp_dir.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init"]);

        let original_git_dir = env::var_os("GIT_DIR");
        let original_git_work_tree = env::var_os("GIT_WORK_TREE");
        unsafe { env::set_var("GIT_DIR", bare_repo.as_os_str()) };
        unsafe { env::remove_var("GIT_WORK_TREE") };

        let hooks_path = resolve_hooks_path(&repo).unwrap();

        match original_git_dir {
            Some(value) => unsafe { env::set_var("GIT_DIR", value) },
            None => unsafe { env::remove_var("GIT_DIR") },
        }
        match original_git_work_tree {
            Some(value) => unsafe { env::set_var("GIT_WORK_TREE", value) },
            None => unsafe { env::remove_var("GIT_WORK_TREE") },
        }

        assert_eq!(hooks_path, repo.join(".git").join("hooks"));
    }
}
