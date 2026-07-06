use crate::{SmeeConfig, platform::Platform};
use std::path::PathBuf;
use thiserror::Error;

mod filesystem;
mod managed_file;
mod script;

pub use filesystem::{FileSystemHookInstaller, write_config_file};
pub use managed_file::{
    MANAGED_FILE_MARKER, has_managed_header, with_managed_header, with_managed_header_with_prefix,
};
pub use script::HookScriptOptions;

use script::render_hook_script;
#[cfg(test)]
use script::shell_single_quote;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Not implemented yet")]
    NotImplemented,
    #[error("Hooks directory not found: {0}")]
    HooksDirNotFound(String),
    #[error("Failed to create hooks directory '{path}': {source}")]
    FailedToCreateHooksDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("No hooks present in the configuration to install")]
    NoHooksPresent,
    #[error("Failed to write hook '{path}': {source}")]
    FailedToWriteHook {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to remove obsolete managed hook '{path}': {source}")]
    FailedToRemoveObsoleteHook {
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
    #[error(
        "Refusing to write managed file through symlink '{path}'. Remove the symlink and retry."
    )]
    RefusingToWriteSymlink { path: String },
    #[error("Failed to read existing file '{path}' while checking managed marker: {source}")]
    FailedToReadExistingFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("The specified configuration path exists but is not a regular file: {path}")]
    ConfigPathNotAFile { path: String },
    #[error("Failed to resolve current executable path: {0}")]
    FailedToResolveCurrentExecutable(std::io::Error),
    #[error("Unsupported managed header prefix '{prefix}'. Expected '#' or 'REM'.")]
    UnsupportedManagedHeaderPrefix { prefix: String },
}

/// Behavioral definition of a hook installer.
///
/// The trait defines a rough shape for anything that might install a hook. However the most common implementation
/// will be a [`FileSystemHookInstaller`]
pub trait HookInstaller {
    fn prepare_install_hooks(&self, hook_names: &[String]) -> Result<(), Error> {
        let _ = hook_names;
        Ok(())
    }

    fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error>;
    fn install_config_file(&self, config_content: &str) -> Result<PathBuf, Error>;

    fn prune_obsolete_hooks(&self, active_hook_names: &[String]) -> Result<(), Error> {
        let _ = active_hook_names;
        Ok(())
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
///         command: "echo pre-commit".into(),
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
    let options = HookScriptOptions::default_for_runtime()?;
    install_hooks_with_options(config, hook_installer, &options)
}

pub fn install_hooks_with_options<T: HookInstaller>(
    config: &SmeeConfig,
    hook_installer: &T,
    options: &HookScriptOptions,
) -> Result<(), Error> {
    if config.hooks.is_empty() {
        return Err(Error::NoHooksPresent);
    }
    let platform = Platform::current();
    let mut phases: Vec<_> = config.hooks.keys().copied().collect();
    phases.sort_by_key(|phase| phase.as_str());
    let active_hook_names: Vec<_> = phases.iter().map(|phase| phase.to_string()).collect();
    hook_installer.prepare_install_hooks(&active_hook_names)?;
    phases
        .into_iter()
        .map(|life_cycle_phase| {
            let lifecycle_phase_kebap = life_cycle_phase.to_string();
            let content = render_hook_script(&platform, &lifecycle_phase_kebap, options);
            let hook_path = hook_installer.install_hook(&lifecycle_phase_kebap, &content)?;
            platform
                .make_executable(&hook_path)
                .map_err(Error::PlatformError)?;
            Ok(())
        })
        .collect::<Result<Vec<_>, Error>>()?;
    hook_installer.prune_obsolete_hooks(&active_hook_names)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::DEFAULT_CONFIG_FILE_NAME;
    use std::sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    };
    use std::{fs, path::Path};

    use super::*;

    struct AssertingHookInstaller {
        assertion: fn(hook_name: &str, hook_content: &str),
        number_of_installed_hooks: AtomicU8,
        number_of_installed_config_files: AtomicU8,
        temp_dir: tempfile::TempDir,
        installed_hook_names: Mutex<Vec<String>>,
    }

    impl AssertingHookInstaller {
        fn new(assertion: fn(hook_name: &str, hook_content: &str)) -> Self {
            Self {
                assertion,
                number_of_installed_hooks: AtomicU8::new(0),
                number_of_installed_config_files: AtomicU8::new(0),
                temp_dir: tempfile::tempdir().unwrap(),
                installed_hook_names: Mutex::new(Vec::new()),
            }
        }

        fn installed_hook_names(&self) -> Vec<String> {
            self.installed_hook_names.lock().unwrap().clone()
        }
    }

    impl HookInstaller for AssertingHookInstaller {
        fn install_hook(&self, hook_name: &str, hook_content: &str) -> Result<PathBuf, Error> {
            (self.assertion)(hook_name, hook_content);
            self.number_of_installed_hooks
                .fetch_add(1, Ordering::SeqCst);
            self.installed_hook_names
                .lock()
                .unwrap()
                .push(hook_name.to_string());
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
                command: "echo Pre-commit hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let options = HookScriptOptions::new(
            PathBuf::from("/tmp/git-smee-bin"),
            PathBuf::from("/tmp/custom-config.toml"),
        );

        let installer = AssertingHookInstaller::new(|hook_name, hook_content| {
            assert_eq!(hook_name, "pre-commit");
            assert!(hook_content.contains("run pre-commit"));
            assert!(hook_content.contains("/tmp/git-smee-bin"));
            assert!(hook_content.contains("/tmp/custom-config.toml"));
        });

        let result = install_hooks_with_options(&config, &installer, &options);
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
                command: "echo Pre-commit hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        hooks_map.insert(
            crate::config::LifeCyclePhase::PrePush,
            vec![crate::config::HookDefinition {
                command: "echo Pre-push hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let options = HookScriptOptions::new(
            PathBuf::from("/tmp/git-smee-bin"),
            PathBuf::from("/tmp/custom-config.toml"),
        );
        let installer = AssertingHookInstaller::new(|hook_name, hook_content| match hook_name {
            "pre-commit" => {
                assert!(hook_content.contains("run pre-commit"));
                assert!(hook_content.contains("/tmp/git-smee-bin"));
            }
            "pre-push" => {
                assert!(hook_content.contains("run pre-push"));
                assert!(hook_content.contains("/tmp/custom-config.toml"));
            }
            _ => panic!("Unexpected hook name: {hook_name}"),
        });
        let result = install_hooks_with_options(&config, &installer, &options);
        assert!(result.is_ok());
        assert_eq!(
            installer.number_of_installed_hooks.load(Ordering::SeqCst),
            2
        );
        assert_eq!(
            installer.installed_hook_names(),
            vec!["pre-commit".to_string(), "pre-push".to_string()]
        );
    }

    #[test]
    fn given_unsorted_hooks_when_installing_then_install_order_is_deterministic() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PrePush,
            vec![crate::config::HookDefinition {
                command: "echo Pre-push hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        hooks_map.insert(
            crate::config::LifeCyclePhase::ApplypatchMsg,
            vec![crate::config::HookDefinition {
                command: "echo Applypatch hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let options = HookScriptOptions::new(
            PathBuf::from("/tmp/git-smee-bin"),
            PathBuf::from("/tmp/custom-config.toml"),
        );
        let installer = AssertingHookInstaller::new(|_, _| {});

        let result = install_hooks_with_options(&config, &installer, &options);

        assert!(result.is_ok());
        assert_eq!(
            installer.installed_hook_names(),
            vec![
                "applypatch-msg".to_string(),
                "pre-commit".to_string(),
                "pre-push".to_string(),
            ]
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
        let managed = with_managed_header_with_prefix(config, "REM").unwrap();

        assert!(managed.starts_with("REM THIS FILE IS MANAGED BY git-smee"));
    }

    #[test]
    fn given_unsupported_prefix_when_adding_managed_header_then_it_returns_error() {
        let result = with_managed_header_with_prefix("echo test", "//");

        assert!(matches!(
            result,
            Err(Error::UnsupportedManagedHeaderPrefix { prefix }) if prefix == "//"
        ));
    }

    #[test]
    fn shell_single_quote_wraps_and_escapes_single_quotes() {
        let path = Path::new("/tmp/it's 100% ready/git-smee");

        assert_eq!(
            shell_single_quote(path),
            "'/tmp/it'\"'\"'s 100% ready/git-smee'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_single_quote_preserves_non_utf8_unix_bytes_with_printf_escape() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let path = PathBuf::from(OsString::from_vec(
            b"/tmp/git-smee-\xFF/config.toml".to_vec(),
        ));

        let escaped = shell_single_quote(&path);

        assert_eq!(
            escaped,
            r"$(printf '%b' '\057\164\155\160\057\147\151\164\055\163\155\145\145\055\377\057\143\157\156\146\151\147\056\164\157\155\154')"
        );
        assert!(!escaped.contains('\u{FFFD}'));
    }

    #[test]
    fn unix_hook_template_does_not_fall_back_to_path_when_embedded_binary_is_stale() {
        let template = Platform::Unix.hook_script_template();

        assert!(!template.contains("command -v git-smee"));
        assert!(!template.contains("git-smee --config"));
        assert!(!template.contains("git smee --config"));
        assert!(template.contains("embedded git-smee executable is not available"));
    }

    #[test]
    fn windows_hook_template_does_not_fall_back_to_path_when_embedded_binary_is_stale() {
        let template = Platform::Windows.hook_script_template();

        assert!(!template.contains("command -v git-smee"));
        assert!(!template.contains("git-smee --config"));
        assert!(!template.contains("git smee --config"));
        assert!(template.contains("embedded git-smee executable is not available"));
    }

    #[test]
    fn windows_hook_template_is_git_for_windows_shell_invokable() {
        let template = Platform::Windows.hook_script_template();

        assert!(template.starts_with("#!/usr/bin/env sh"));
        assert!(template.contains("GIT_SMEE_BIN_WIN={git_smee_executable}"));
        assert!(template.contains("GIT_SMEE_CONFIG={config_path}"));
        assert!(template.contains("cygpath -u \"$GIT_SMEE_BIN_WIN\""));
        assert!(template.contains("run {hook} \"$@\""));
        assert!(!template.contains("@echo off"));
        assert!(!template.contains("%*"));
    }

    #[cfg(unix)]
    #[test]
    fn given_special_paths_when_installing_hooks_then_unix_hook_contains_escaped_values() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let options = HookScriptOptions::new(
            PathBuf::from("/tmp/it's 100% ready/git-smee"),
            PathBuf::from("/tmp/configs/it's 100% ready.toml"),
        );
        let installer = AssertingHookInstaller::new(|hook_name, hook_content| {
            assert_eq!(hook_name, "pre-commit");
            assert!(hook_content.contains("GIT_SMEE_BIN='/tmp/it'\"'\"'s 100% ready/git-smee'"));
            assert!(
                hook_content.contains("GIT_SMEE_CONFIG='/tmp/configs/it'\"'\"'s 100% ready.toml'")
            );
        });

        let result = install_hooks_with_options(&config, &installer, &options);
        assert!(result.is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn given_special_paths_when_installing_hooks_then_windows_hook_contains_escaped_values() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            crate::config::LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook".into(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let options = HookScriptOptions::new(
            PathBuf::from(r#"C:\Program Files\100%"quoted"\git-smee.exe"#),
            PathBuf::from(r#"C:\repo\configs\it's 100% "ready".toml"#),
        );
        let installer = AssertingHookInstaller::new(|hook_name, hook_content| {
            assert_eq!(hook_name, "pre-commit");
            assert!(hook_content.starts_with("#!/usr/bin/env sh"));
            assert!(
                hook_content
                    .contains(r#"GIT_SMEE_BIN_WIN='C:\Program Files\100%"quoted"\git-smee.exe'"#)
            );
            assert!(
                hook_content.contains(
                    "GIT_SMEE_CONFIG='C:\\repo\\configs\\it'\"'\"'s 100% \"ready\".toml'"
                )
            );
        });

        let result = install_hooks_with_options(&config, &installer, &options);
        assert!(result.is_ok());
    }
}
