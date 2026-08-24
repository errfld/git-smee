use std::{
    path::Path,
    process::{Command, ExitStatus},
};

use super::Error;

pub(super) trait GitClient {
    fn rev_parse_bool(
        &self,
        current_dir: &Path,
        flag: &str,
    ) -> Result<GitCommandResult<bool>, Error>;

    fn rev_parse_path_bytes(
        &self,
        current_dir: &Path,
        flag: &str,
    ) -> Result<GitCommandResult<Vec<u8>>, Error>;

    fn git_path_bytes(
        &self,
        repository_root: &Path,
        git_path: &str,
    ) -> Result<GitCommandResult<Vec<u8>>, Error>;
}

pub(super) struct RealGitClient;

impl GitClient for RealGitClient {
    fn rev_parse_bool(
        &self,
        current_dir: &Path,
        flag: &str,
    ) -> Result<GitCommandResult<bool>, Error> {
        match run_git_command(git_rev_parse_command(current_dir, flag))? {
            GitCommandResult::Success(stdout) => Ok(GitCommandResult::Success(
                String::from_utf8_lossy(&stdout).trim() == "true",
            )),
            GitCommandResult::Failure(failure) => Ok(GitCommandResult::Failure(failure)),
        }
    }

    fn rev_parse_path_bytes(
        &self,
        current_dir: &Path,
        flag: &str,
    ) -> Result<GitCommandResult<Vec<u8>>, Error> {
        run_git_command(git_rev_parse_command(current_dir, flag))
    }

    fn git_path_bytes(
        &self,
        repository_root: &Path,
        git_path: &str,
    ) -> Result<GitCommandResult<Vec<u8>>, Error> {
        let mut command = git_command_with_explicit_repo(repository_root);
        command.arg("rev-parse").arg("--git-path").arg(git_path);
        run_git_command(command)
    }
}

fn git_rev_parse_command(current_dir: &Path, flag: &str) -> Command {
    let mut command = Command::new("git");
    command.current_dir(current_dir).arg("rev-parse").arg(flag);
    command
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

fn run_git_command(mut command: Command) -> Result<GitCommandResult<Vec<u8>>, Error> {
    let output = command.output().map_err(Error::FailedToExecuteGit)?;

    if !output.status.success() {
        return Ok(GitCommandResult::Failure(GitCommandFailure::from_output(
            &output.stderr,
            output.status,
        )));
    }

    Ok(GitCommandResult::Success(output.stdout))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GitCommandResult<T> {
    Success(T),
    Failure(GitCommandFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitCommandFailure {
    pub(super) status_code: Option<i32>,
    pub(super) stderr: String,
}

impl GitCommandFailure {
    fn from_output(stderr: &[u8], status: ExitStatus) -> Self {
        Self {
            status_code: status.code(),
            stderr: stderr_or_status(stderr, status.code()),
        }
    }
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

#[cfg(test)]
pub(super) mod test_support {
    use std::path::Path;

    use super::git_command_with_explicit_repo;

    pub(in crate::repository) fn git(repo: &Path, args: &[&str]) {
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

    pub(in crate::repository) fn git_output(repo: &Path, args: &[&str]) -> String {
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
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, env, fs, path::Path};

    use crate::test_support::process_state_lock;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn explicit_repository_command_clears_only_repository_context_variables() {
        let command = git_command_with_explicit_repo(Path::new("repository"));
        let removed: BTreeSet<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            removed,
            BTreeSet::from([
                "GIT_COMMON_DIR".to_string(),
                "GIT_DIR".to_string(),
                "GIT_INDEX_FILE".to_string(),
                "GIT_OBJECT_DIRECTORY".to_string(),
                "GIT_WORK_TREE".to_string(),
            ])
        );
    }

    #[test]
    fn failure_without_stderr_uses_exit_status() {
        assert_eq!(
            stderr_or_status(b"\n", Some(128)),
            "git exited with status 128"
        );
    }

    #[test]
    fn failure_without_status_or_stderr_reports_signal() {
        assert_eq!(stderr_or_status(b"", None), "git terminated by signal");
    }

    #[test]
    fn explicit_repository_execution_ignores_ambient_git_directory() {
        let _guard = process_state_lock();
        let temp_dir = TempDir::new().unwrap();
        let bare_repo = temp_dir.path().join("remote.git");
        fs::create_dir(&bare_repo).unwrap();
        test_support::git(&bare_repo, &["init", "--bare"]);

        let repository = temp_dir.path().join("repository");
        fs::create_dir(&repository).unwrap();
        test_support::git(&repository, &["init"]);

        let original_git_dir = env::var_os("GIT_DIR");
        let original_git_work_tree = env::var_os("GIT_WORK_TREE");
        unsafe { env::set_var("GIT_DIR", bare_repo.as_os_str()) };
        unsafe { env::remove_var("GIT_WORK_TREE") };

        let result = RealGitClient.git_path_bytes(&repository, "hooks");

        match original_git_dir {
            Some(value) => unsafe { env::set_var("GIT_DIR", value) },
            None => unsafe { env::remove_var("GIT_DIR") },
        }
        match original_git_work_tree {
            Some(value) => unsafe { env::set_var("GIT_WORK_TREE", value) },
            None => unsafe { env::remove_var("GIT_WORK_TREE") },
        }

        assert!(matches!(result, Ok(GitCommandResult::Success(_))));
    }
}
