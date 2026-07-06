use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{DEFAULT_CONFIG_FILE_NAME, config::LifeCyclePhase};

use super::managed_file::{ensure_can_write_config_file, ensure_not_symlink, is_managed_file};
use super::{Error, HookInstaller};

pub struct FileSystemHookInstaller {
    repository_root: PathBuf,
    hooks_dir: PathBuf,
    force_overwrite: bool,
}

impl FileSystemHookInstaller {
    /// Git path key used to resolve the effective hooks directory.
    pub const HOOKS_GIT_PATH_KEY: &str = "hooks";

    /// Creates a hook installer rooted at the current working directory.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use git_smee_core::installer::FileSystemHookInstaller;
    /// use std::{env, process::Command};
    /// use tempfile::tempdir;
    ///
    /// let temp_dir = tempdir().unwrap();
    /// Command::new("git")
    ///     .arg("init")
    ///     .current_dir(temp_dir.path())
    ///     .output()
    ///     .unwrap();
    ///
    /// let original_dir = env::current_dir().unwrap();
    /// env::set_current_dir(temp_dir.path()).unwrap();
    ///
    /// let installer = FileSystemHookInstaller::new().unwrap();
    ///
    /// env::set_current_dir(&original_dir).unwrap();
    /// assert!(installer.effective_hooks_dir().exists());
    /// drop(installer);
    /// ```
    pub fn new() -> Result<Self, Error> {
        Self::from_default()
    }

    /// Creates a hook installer using `./` as the repository root.
    pub fn from_default() -> Result<Self, Error> {
        Self::from_default_with_force(false)
    }

    /// Creates a hook installer using `./` as the repository root and a
    /// configurable overwrite policy.
    pub fn from_default_with_force(force_overwrite: bool) -> Result<Self, Error> {
        Self::from_path_with_force(PathBuf::from("./"), force_overwrite)
    }

    /// Creates a `FileSystemHookInstaller` rooted at the provided repository path.
    pub fn from_path(repository_root: PathBuf) -> Result<Self, Error> {
        Self::from_path_with_force(repository_root, false)
    }

    /// Creates a `FileSystemHookInstaller` rooted at the provided repository path and
    /// with explicit overwrite behavior.
    pub fn from_path_with_force(
        repository_root: PathBuf,
        force_overwrite: bool,
    ) -> Result<Self, Error> {
        let repository_root =
            repository_root
                .canonicalize()
                .map_err(|source| Error::InvalidRepositoryRoot {
                    path: repository_root.to_string_lossy().to_string(),
                    source,
                })?;
        let hooks_path =
            crate::repository::resolve_git_path(&repository_root, Self::HOOKS_GIT_PATH_KEY)?;
        if !hooks_path.exists() {
            fs::create_dir_all(&hooks_path).map_err(|source| Error::FailedToCreateHooksDir {
                path: hooks_path.to_string_lossy().to_string(),
                source,
            })?;
        }
        if !hooks_path.is_dir() {
            return Err(Error::HooksDirNotFound(
                hooks_path.to_string_lossy().to_string(),
            ));
        }
        Ok(Self {
            repository_root,
            hooks_dir: hooks_path,
            force_overwrite,
        })
    }

    pub fn effective_hooks_dir(&self) -> &PathBuf {
        &self.hooks_dir
    }

    pub fn ensure_can_write_managed_config(
        config_file: &Path,
        force_overwrite: bool,
    ) -> Result<(), Error> {
        ensure_not_symlink(config_file)?;

        if !config_file.exists() || force_overwrite {
            return Ok(());
        }

        let path = config_file.to_string_lossy().to_string();
        if is_managed_file(config_file)? {
            return Err(Error::RefusingToOverwriteManagedConfigFile { path });
        }

        Err(Error::RefusingToOverwriteUnmanagedConfigFile { path })
    }

    fn ensure_can_write_hook(&self, hook_file: &Path) -> Result<(), Error> {
        ensure_not_symlink(hook_file)?;

        if !hook_file.exists() || self.force_overwrite {
            return Ok(());
        }

        if is_managed_file(hook_file)? {
            return Ok(());
        }

        Err(Error::RefusingToOverwriteUnmanagedHookFile {
            path: hook_file.to_string_lossy().to_string(),
        })
    }

    fn ensure_can_write_config(&self, config_file: &Path) -> Result<(), Error> {
        ensure_can_write_config_file(config_file, self.force_overwrite)
    }

    fn prune_obsolete_managed_hook(
        &self,
        hook_name: &str,
        active_hook_names: &[String],
    ) -> Result<(), Error> {
        if active_hook_names
            .iter()
            .any(|active_hook| active_hook == hook_name)
        {
            return Ok(());
        }

        let hook_file = self.hooks_dir.join(hook_name);
        if !hook_file.exists() || !is_managed_file(&hook_file)? {
            return Ok(());
        }

        fs::remove_file(&hook_file).map_err(|source| Error::FailedToRemoveObsoleteHook {
            path: hook_file.to_string_lossy().to_string(),
            source,
        })
    }
}

impl HookInstaller for FileSystemHookInstaller {
    fn prepare_install_hooks(&self, hook_names: &[String]) -> Result<(), Error> {
        for hook_name in hook_names {
            let hook_file = self.hooks_dir.join(hook_name);
            self.ensure_can_write_hook(&hook_file)?;
        }
        Ok(())
    }

    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error> {
        let hook_file = self.hooks_dir.join(hook_name);
        self.ensure_can_write_hook(&hook_file)?;
        atomic_write_file(&hook_file, hook_content).map_err(|source| Error::FailedToWriteHook {
            path: hook_file.to_string_lossy().to_string(),
            source,
        })?;
        Ok(hook_file)
    }

    fn install_config_file(&self, config_content: &str) -> Result<PathBuf, Error> {
        let config_path = self.repository_root.join(DEFAULT_CONFIG_FILE_NAME);
        self.ensure_can_write_config(&config_path)?;
        atomic_write_file(&config_path, config_content).map_err(|source| {
            Error::FailedToWriteConfigFile {
                path: config_path.to_string_lossy().to_string(),
                source,
            }
        })?;
        Ok(config_path)
    }

    fn prune_obsolete_hooks(&self, active_hook_names: &[String]) -> Result<(), Error> {
        for phase in LifeCyclePhase::all() {
            self.prune_obsolete_managed_hook(phase.as_str(), active_hook_names)?;
        }
        Ok(())
    }
}

/// Writes a git-smee config file at an arbitrary path using the same managed/unmanaged
/// overwrite semantics as [`FileSystemHookInstaller::install_config_file`].
pub fn write_config_file(
    config_path: &Path,
    config_content: &str,
    force_overwrite: bool,
) -> Result<(), Error> {
    ensure_can_write_config_file(config_path, force_overwrite)?;
    match config_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|source| Error::FailedToWriteConfigFile {
                path: config_path.to_string_lossy().to_string(),
                source,
            })?;
        }
        _ => {}
    }
    atomic_write_file(config_path, config_content).map_err(|source| {
        Error::FailedToWriteConfigFile {
            path: config_path.to_string_lossy().to_string(),
            source,
        }
    })
}

fn atomic_write_file(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp_file = tempfile::Builder::new()
        .prefix(".git-smee-")
        .suffix(".tmp")
        .tempfile_in(parent)?;

    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    temp_file.persist(path).map_err(|error| error.error)?;
    sync_parent_dir(parent)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent_dir(parent: &Path) -> std::io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_dir(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}
