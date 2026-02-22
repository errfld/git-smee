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

/// Finds the git repository root by walking up from the current directory
/// looking for a `.git` directory.
///
/// # Examples
///
/// ```rust
/// use git_smee_core::find_git_root;
/// use std::{env, fs};
/// use tempfile::tempdir;
///
/// let temp_dir = tempdir().unwrap();
/// let git_dir = temp_dir.path().join(".git");
/// fs::create_dir(&git_dir).unwrap();
/// let nested = temp_dir.path().join("nested");
/// fs::create_dir(&nested).unwrap();
///
/// let original_dir = env::current_dir().unwrap();
/// env::set_current_dir(&nested).unwrap();
///
/// let repo_root = find_git_root().unwrap();
///
/// env::set_current_dir(&original_dir).unwrap();
/// assert!(repo_root.join(".git").exists());
/// ```
pub fn find_git_root() -> Result<PathBuf, Error> {
    let current = env::current_dir().map_err(Error::FailedToChangeDirectory)?;
    find_git_root_from(&current, None)
}

fn find_git_root_from(start: &Path, stop_at: Option<&Path>) -> Result<PathBuf, Error> {
    let mut current = start.to_path_buf();
    loop {
        let git_dir = current.join(".git");
        if git_dir.exists() {
            return Ok(current);
        }

        if stop_at.is_some_and(|limit| current == limit) {
            return Err(Error::NotInGitRepository);
        }

        if !current.pop() {
            // Reached filesystem root without finding .git
            return Err(Error::NotInGitRepository);
        }
    }
}

/// Validates that we're in a git repository and changes to the repository root.
///
/// # Examples
///
/// ```rust
/// use git_smee_core::ensure_in_repo_root;
/// use std::{env, fs};
/// use tempfile::tempdir;
///
/// let temp_dir = tempdir().unwrap();
/// let git_dir = temp_dir.path().join(".git");
/// fs::create_dir(&git_dir).unwrap();
/// let nested = temp_dir.path().join("nested");
/// fs::create_dir(&nested).unwrap();
///
/// let original_dir = env::current_dir().unwrap();
/// env::set_current_dir(&nested).unwrap();
///
/// ensure_in_repo_root().unwrap();
/// let current_dir = env::current_dir().unwrap();
///
/// env::set_current_dir(&original_dir).unwrap();
/// assert!(current_dir.join(".git").exists());
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
        let git_dir = temp_dir.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        // Verify that the result contains the .git directory
        let result_path = result.unwrap();
        assert!(result_path.join(".git").exists());
    }

    #[test]
    fn given_current_dir_is_subdirectory_of_repo_when_finding_root_then_returns_repo_root() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let git_dir = temp_dir.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&sub_dir).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        // Verify that the result contains the .git directory
        let result_path = result.unwrap();
        assert!(result_path.join(".git").exists());
    }

    #[test]
    fn given_current_dir_is_deeply_nested_subdirectory_when_finding_root_then_returns_repo_root() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let git_dir = temp_dir.path().join(".git");
        fs::create_dir(&git_dir).unwrap();

        // Create nested directories: a/b/c
        let nested_path = temp_dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested_path).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&nested_path).unwrap();

        let result = find_git_root();

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        // Verify that the result contains the .git directory
        let result_path = result.unwrap();
        assert!(result_path.join(".git").exists());
    }

    #[test]
    fn given_not_in_git_repo_when_finding_root_then_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let nested = temp_dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();

        let result = find_git_root_from(&nested, Some(temp_dir.path()));

        assert!(matches!(result, Err(Error::NotInGitRepository)));
    }

    #[test]
    fn given_in_git_repo_when_ensuring_in_repo_root_then_succeeds() {
        let _guard = CWD_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let git_dir = temp_dir.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
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
        // Verify that we moved to a directory that contains .git
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
