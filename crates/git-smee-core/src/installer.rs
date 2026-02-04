use crate::{DEFAULT_CONFIG_FILE_NAME, SmeeConfig, platform::Platform};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Marker string used to identify files managed by git-smee.
pub const MANAGED_FILE_MARKER: &str = "THIS FILE IS MANAGED BY git-smee";

/// Prefixes content with a managed marker using `#` comments.
///
/// If content starts with a shebang (`#!`), the marker is inserted after the shebang
/// so script executability is preserved.
pub fn with_managed_header(content: &str) -> String {
    with_managed_header_with_prefix(content, "#")
}

/// Prefixes content with a managed marker using the provided comment prefix.
///
/// If content starts with a shebang (`#!`), the marker is inserted after the shebang
/// so script executability is preserved.
pub fn with_managed_header_with_prefix(content: &str, comment_prefix: &str) -> String {
    let marker_line = format!("{comment_prefix} {MANAGED_FILE_MARKER}");
    if content.starts_with("#!") {
        if let Some(shebang_end) = content.find('\n') {
            let (shebang, rest) = content.split_at(shebang_end + 1);
            return format!("{shebang}{marker_line}\n\n{rest}");
        }

        return format!("{content}\n{marker_line}\n\n");
    }

    format!("{marker_line}\n\n{content}")
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Not implemented yet")]
    NotImplemented,
    #[error("Hooks directory not found: {0}")]
    HooksDirNotFound(String),
    #[error("No hooks present in the configuration to install")]
    NoHooksPresent,
    #[error("Failed to write hook '{path}': {source}")]
    FailedToWriteHook {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to write config file '{path}': {source}")]
    FailedToWriteConfigFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    // add installer-specific errors here later
    #[error("A platform-specific error occurred: {0}")]
    PlatformError(#[from] crate::platform::Error),
    #[error("Failed to resolve the hooks directory: {0}")]
    FailedToResolveHooksDirectory(#[from] crate::repository::Error),
    #[error("Invalid repository root '{path}': {source}")]
    InvalidRepositoryRoot {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "Refusing to overwrite unmanaged hook file '{path}'. Re-run with --force to overwrite."
    )]
    RefusingToOverwriteUnmanagedHookFile { path: String },
    #[error(
        "Refusing to overwrite existing unmanaged config file '{path}'. Re-run with --force to overwrite."
    )]
    RefusingToOverwriteUnmanagedConfigFile { path: String },
    #[error(
        "Refusing to overwrite existing managed config file '{path}'. Re-run with --force to overwrite."
    )]
    RefusingToOverwriteManagedConfigFile { path: String },
    #[error("Failed to read existing file '{path}' while checking managed marker: {source}")]
    FailedToReadExistingFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Behavioral definition of a hook installer.
///
/// The trait defines a rough shape for anything that might install a hook. However the most common implementation
/// will be a [`FileSystemHookInstaller`]
pub trait HookInstaller {
    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error>;
    fn install_config_file(&self, config_content: &str) -> Result<PathBuf, Error>;
}

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
    /// let installer = FileSystemHookInstaller::from_default().unwrap();
    ///
    /// env::set_current_dir(&original_dir).unwrap();
    /// assert!(installer.effective_hooks_dir().exists());
    /// drop(installer);
    /// ```
    pub fn from_default() -> Result<Self, Error> {
        Self::from_default_with_force(false)
    }

    /// Creates a hook installer using `./` as the repository root and a
    /// configurable overwrite policy.
    pub fn from_default_with_force(force_overwrite: bool) -> Result<Self, Error> {
        Self::from_path_with_force(PathBuf::from("./"), force_overwrite)
    }

    /// Creates a `FileSystemHookInstaller` rooted at the provided repository path.
    ///
    /// The hooks directory must exist within the provided root.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use git_smee_core::installer::FileSystemHookInstaller;
    /// use std::process::Command;
    /// use tempfile::tempdir;
    ///
    /// let temp_dir = tempdir().unwrap();
    /// Command::new("git")
    ///     .arg("init")
    ///     .current_dir(temp_dir.path())
    ///     .output()
    ///     .unwrap();
    ///
    /// let installer = FileSystemHookInstaller::from_path(temp_dir.path().to_path_buf()).unwrap();
    /// assert!(installer.effective_hooks_dir().exists());
    /// drop(installer);
    /// ```
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
        if !hooks_path.exists() || !hooks_path.is_dir() {
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

    fn ensure_can_write_hook(&self, hook_file: &Path) -> Result<(), Error> {
        if !hook_file.exists() || self.force_overwrite {
            return Ok(());
        }

        if Self::is_managed_file(hook_file)? {
            return Ok(());
        }

        Err(Error::RefusingToOverwriteUnmanagedHookFile {
            path: hook_file.to_string_lossy().to_string(),
        })
    }

    fn ensure_can_write_config(&self, config_file: &Path) -> Result<(), Error> {
        if !config_file.exists() || self.force_overwrite {
            return Ok(());
        }

        let path = config_file.to_string_lossy().to_string();
        if Self::is_managed_file(config_file)? {
            return Err(Error::RefusingToOverwriteManagedConfigFile { path });
        }

        Err(Error::RefusingToOverwriteUnmanagedConfigFile { path })
    }

    fn is_managed_file(path: &Path) -> Result<bool, Error> {
        let mut file = fs::File::open(path).map_err(|source| Error::FailedToReadExistingFile {
            path: path.to_string_lossy().to_string(),
            source,
        })?;
        let mut header_buf = [0_u8; 1024];
        let bytes_read =
            file.read(&mut header_buf)
                .map_err(|source| Error::FailedToReadExistingFile {
                    path: path.to_string_lossy().to_string(),
                    source,
                })?;
        let header = &header_buf[..bytes_read];
        let marker_hash = format!("# {MANAGED_FILE_MARKER}");
        let marker_rem = format!("REM {MANAGED_FILE_MARKER}");

        for line in header.split(|byte| *byte == b'\n').take(8) {
            let normalized_line = line.strip_suffix(b"\r").unwrap_or(line);
            if normalized_line.is_empty() {
                continue;
            }
            if normalized_line == marker_hash.as_bytes() || normalized_line == marker_rem.as_bytes()
            {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl HookInstaller for FileSystemHookInstaller {
    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error> {
        let hook_file = self.hooks_dir.join(hook_name);
        self.ensure_can_write_hook(&hook_file)?;
        fs::write(&hook_file, hook_content).map_err(|source| Error::FailedToWriteHook {
            path: hook_file.to_string_lossy().to_string(),
            source,
        })?;
        Ok(hook_file)
    }

    fn install_config_file(&self, config_content: &str) -> Result<PathBuf, Error> {
        let config_path = self.repository_root.join(DEFAULT_CONFIG_FILE_NAME);
        self.ensure_can_write_config(&config_path)?;
        fs::write(&config_path, config_content).map_err(|source| {
            Error::FailedToWriteConfigFile {
                path: config_path.to_string_lossy().to_string(),
                source,
            }
        })?;
        Ok(config_path)
    }
}

/// Installs hook scripts for each configured lifecycle phase.
///
/// # Examples
///
/// ```rust
/// use git_smee_core::{install_hooks, SmeeConfig};
/// use git_smee_core::config::{HookDefinition, LifeCyclePhase};
/// use git_smee_core::installer::FileSystemHookInstaller;
/// use std::{fs, process::Command};
/// use tempfile::tempdir;
///
/// let temp_dir = tempdir().unwrap();
/// Command::new("git")
///     .arg("init")
///     .current_dir(temp_dir.path())
///     .output()
///     .unwrap();
/// let hooks_dir = temp_dir.path().join(".git").join("hooks");
///
/// let mut hooks = std::collections::HashMap::new();
/// hooks.insert(
///     LifeCyclePhase::PreCommit,
///     vec![HookDefinition {
///         command: "echo pre-commit".to_string(),
///         parallel_execution_allowed: false,
///     }],
/// );
/// let config = SmeeConfig { hooks };
///
/// let installer = FileSystemHookInstaller::from_path(temp_dir.path().to_path_buf()).unwrap();
/// install_hooks(&config, &installer).unwrap();
///
/// assert!(hooks_dir.join("pre-commit").exists());
/// ```
pub fn install_hooks<T: HookInstaller>(
    config: &SmeeConfig,
    hook_installer: &T,
) -> Result<(), Error> {
    if config.hooks.is_empty() {
        return Err(Error::NoHooksPresent);
    }
    let platform = Platform::current();
    config
        .hooks
        .keys()
        .map(|life_cycle_phase| {
            let lifecycle_phase_kebap = life_cycle_phase.to_string();
            let content = platform
                .hook_script_template()
                .replace("{hook}", &lifecycle_phase_kebap);
            let hook_path = hook_installer.install_hook(&lifecycle_phase_kebap, &content)?;
            platform
                .make_executable(&hook_path)
                .map_err(Error::PlatformError)?;
            Ok(())
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use super::*;

    struct AssertingHookInstaller {
        assertion: fn(hook_name: &str, hook_content: &str) -> (),
        number_of_installed_hooks: AtomicU8,
        number_of_installed_config_files: AtomicU8,
        temp_dir: tempfile::TempDir,
    }

    impl AssertingHookInstaller {
        fn new(assertion: fn(hook_name: &str, hook_content: &str) -> ()) -> Self {
            Self {
                assertion,
                number_of_installed_hooks: AtomicU8::new(0),
                number_of_installed_config_files: AtomicU8::new(0),
                temp_dir: tempfile::tempdir().unwrap(),
            }
        }
    }

    impl HookInstaller for AssertingHookInstaller {
        fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error> {
            (self.assertion)(hook_name, hook_content);
            self.number_of_installed_hooks
                .fetch_add(1, Ordering::SeqCst);
            let hook = self.temp_dir.path().join(hook_name);
            fs::write(&hook, hook_content).unwrap();
            Ok(hook)
        }

        fn install_config_file(&self, config_content: &str) -> Result<PathBuf, Error> {
            self.number_of_installed_config_files
                .fetch_add(1, Ordering::SeqCst);
            let config_file = self.temp_dir.path().join(DEFAULT_CONFIG_FILE_NAME);
            fs::write(&config_file, config_content).unwrap();
            Ok(config_file)
        }
    }

    #[test]
    fn given_empty_smee_config_when_installing_hooks_then_no_hooks_present_error() {
        let config = SmeeConfig {
            hooks: std::collections::HashMap::new(),
        };

        let installer = AssertingHookInstaller::new(|_, _| panic!("No hooks should be installed"));

        let result = install_hooks(&config, &installer);
        assert!(matches!(result, Err(Error::NoHooksPresent)));
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn given_single_hook_when_installing_hooks_then_hook_installed() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };

        let installer = AssertingHookInstaller::new(|hook_name, hook_content| {
            assert_eq!(hook_name, "pre-commit");
            assert!(hook_content.contains("git smee run pre-commit"));
        });

        let result = install_hooks(&config, &installer);
        if let Err(err) = &result {
            println!("Error installing hooks: {err:?}");
        }
        assert!(result.is_ok());
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn given_multiple_hooks_when_installing_hooks_then_all_hooks_installed() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        hooks_map.insert(
            crate::config::LifeCyclePhase::PrePush,
            vec![crate::config::HookDefinition {
                command: "echo Pre-push hook".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let installer = AssertingHookInstaller::new(|hook_name, hook_content| match hook_name {
            "pre-commit" => {
                assert!(hook_content.contains("git smee run pre-commit"));
            }
            "pre-push" => {
                assert!(hook_content.contains("git smee run pre-push"));
            }
            _ => panic!("Unexpected hook name: {hook_name}"),
        });
        let result = install_hooks(&config, &installer);
        assert!(result.is_ok());
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            2
        );
    }

    #[test]
    fn when_initializing_config_file_then_config_written() {
        let installer = AssertingHookInstaller::new(|_, _| {});
        let serialized_config: String = (&SmeeConfig::default()).try_into().unwrap();
        let install_result = installer.install_config_file(&serialized_config);
        assert!(install_result.is_ok());
        assert_eq!(
            installer
                .number_of_installed_config_files
                .load(Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn given_content_when_adding_managed_header_then_marker_is_present() {
        let config = "[[pre-commit]]\ncommand = \"cargo test\"";
        let managed = with_managed_header(config);

        assert!(managed.contains(MANAGED_FILE_MARKER));
        assert!(managed.contains(config));
    }

    #[test]
    fn given_shebang_content_when_adding_managed_header_then_shebang_stays_first_line() {
        let script = "#!/usr/bin/env sh\necho test\n";
        let managed = with_managed_header(script);

        let mut lines = managed.lines();
        assert_eq!(lines.next(), Some("#!/usr/bin/env sh"));
        assert_eq!(lines.next(), Some("# THIS FILE IS MANAGED BY git-smee"));
    }

    #[test]
    fn given_shebang_without_newline_when_adding_managed_header_then_shebang_stays_first_line() {
        let script = "#!/usr/bin/env sh";
        let managed = with_managed_header(script);

        let mut lines = managed.lines();
        assert_eq!(lines.next(), Some("#!/usr/bin/env sh"));
        assert_eq!(lines.next(), Some("# THIS FILE IS MANAGED BY git-smee"));
    }

    #[test]
    fn given_custom_prefix_when_adding_managed_header_then_prefix_is_used() {
        let config = "[[pre-commit]]\ncommand = \"cargo test\"";
        let managed = with_managed_header_with_prefix(config, "REM");

        assert!(managed.starts_with("REM THIS FILE IS MANAGED BY git-smee"));
    }
}
