use std::{
    env,
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
/// assert_eq!(repo_root, temp_dir.path().canonicalize().unwrap());
/// ```
pub fn find_git_root() -> Result<PathBuf, Error> {
    let current_dir = env::current_dir().map_err(Error::FailedToChangeDirectory)?;

    if git_rev_parse_bool("--is-inside-work-tree")?
        && let Some(root) = git_rev_parse_value("--show-toplevel")?
    {
        return Ok(PathBuf::from(root));
    }

    if git_rev_parse_bool("--is-bare-repository")? {
        return current_dir
            .canonicalize()
            .map_err(Error::FailedToChangeDirectory);
    }

    Err(Error::NotInGitRepository)
}

fn git_rev_parse_bool(flag: &str) -> Result<bool, Error> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg(flag)
        .output()
        .map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        return Ok(false);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn git_rev_parse_value(flag: &str) -> Result<Option<String>, Error> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg(flag)
        .output()
        .map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        return Ok(None);
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value))
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
/// assert_eq!(current_dir, temp_dir.path().canonicalize().unwrap());
/// ```
pub fn ensure_in_repo_root() -> Result<(), Error> {
    let git_root = find_git_root()?;
    env::set_current_dir(&git_root).map_err(Error::FailedToChangeDirectory)
}

/// Resolves a Git path (as interpreted by `git rev-parse --git-path`) from the
/// given repository root.
pub fn resolve_git_path(repository_root: &Path, git_path: &str) -> Result<PathBuf, Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
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

    let raw_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw_path.is_empty() {
        return Err(Error::EmptyGitPath {
            repository_root: repository_root.display().to_string(),
            git_path: git_path.to_string(),
        });
    }

    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(repository_root.join(path))
    }
}

/// Resolves the effective hooks directory used by Git for the repository.
pub fn resolve_hooks_path(repository_root: &Path) -> Result<PathBuf, Error> {
    resolve_git_path(repository_root, "hooks")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command, sync::Mutex};
    use tempfile::TempDir;

    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn given_current_dir_is_git_root_when_finding_root_then_returns_current_dir() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init"]);

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        let result_path = result.unwrap();
        assert_eq!(result_path, temp_dir.path().canonicalize().unwrap());
    }

    #[test]
    fn given_current_dir_is_subdirectory_of_repo_when_finding_root_then_returns_repo_root() {
        let _guard = CWD_MUTEX.lock().unwrap();
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
        assert_eq!(result_path, temp_dir.path().canonicalize().unwrap());
    }

    #[test]
    fn given_current_dir_is_deeply_nested_subdirectory_when_finding_root_then_returns_repo_root() {
        let _guard = CWD_MUTEX.lock().unwrap();
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
        assert_eq!(result_path, temp_dir.path().canonicalize().unwrap());
    }

    #[test]
    fn given_not_in_git_repo_when_finding_root_then_returns_error() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(matches!(result, Err(Error::NotInGitRepository)));
    }

    #[test]
    fn given_bare_repo_when_finding_root_then_returns_current_dir() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init", "--bare"]);

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), temp_dir.path().canonicalize().unwrap());
    }

    #[test]
    fn given_in_git_repo_when_ensuring_in_repo_root_then_succeeds() {
        let _guard = CWD_MUTEX.lock().unwrap();
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
        assert_eq!(current, temp_dir.path().canonicalize().unwrap());
        assert!(git_exists);
    }

    #[test]
    fn given_not_in_git_repo_when_ensuring_in_repo_root_then_returns_error() {
        let _guard = CWD_MUTEX.lock().unwrap();
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
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        git(temp_dir.path(), &["init", "--bare"]);
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = ensure_in_repo_root();
        let current = env::current_dir().unwrap();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(current, temp_dir.path().canonicalize().unwrap());
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

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
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
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
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
}
