use std::{
    env,
    ffi::OsStr,
    io,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use git_smee_core::{DEFAULT_CONFIG_FILE_NAME, SmeeConfig, config};

pub(crate) fn resolve_config_path(cli_config: Option<PathBuf>, invocation_dir: &Path) -> PathBuf {
    if let Some(path) = cli_config {
        return normalize_user_config_path(path, invocation_dir);
    }
    match env::var_os("GIT_SMEE_CONFIG") {
        Some(path_from_env) if !is_blank_env_config(&path_from_env) => {
            return normalize_user_config_path(PathBuf::from(path_from_env), invocation_dir);
        }
        _ => {}
    }
    PathBuf::from_str(DEFAULT_CONFIG_FILE_NAME).expect("default config path should be valid")
}

fn is_blank_env_config(value: &OsStr) -> bool {
    value.to_str().is_some_and(|value| value.trim().is_empty())
}

fn normalize_user_config_path(path: PathBuf, invocation_dir: &Path) -> PathBuf {
    let path = expand_user_home_path(path);
    if path.is_absolute() {
        path
    } else {
        invocation_dir.join(path)
    }
}

#[cfg(unix)]
fn expand_user_home_path(path: PathBuf) -> PathBuf {
    let Some(home_dir) = env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return path;
    };
    let mut components = path.components();
    let Some(first) = components.next() else {
        return path;
    };
    if first.as_os_str() != "~" {
        return path;
    }

    let mut expanded = PathBuf::from(home_dir);
    for component in components {
        expanded.push(component.as_os_str());
    }
    expanded
}

#[cfg(not(unix))]
fn expand_user_home_path(path: PathBuf) -> PathBuf {
    path
}

pub(crate) fn read_config_file(config_path: &Path) -> Result<SmeeConfig, config::Error> {
    config::SmeeConfig::try_from(config_path)
}

pub(crate) fn is_default_config_path(config_path: &Path, repository_root: &Path) -> bool {
    if config_path == Path::new(DEFAULT_CONFIG_FILE_NAME)
        || config_path == repository_root.join(DEFAULT_CONFIG_FILE_NAME)
    {
        return true;
    }

    let default_config_path = repository_root.join(DEFAULT_CONFIG_FILE_NAME);
    if let (Ok(config_path), Ok(default_config_path)) = (
        config_path.canonicalize(),
        default_config_path.canonicalize(),
    ) {
        return config_path == default_config_path;
    }

    normalize_path_lexically(config_path) == normalize_path_lexically(&default_config_path)
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn normalize_config_path_for_hook_script(
    config_path: &Path,
    repository_root: &Path,
) -> io::Result<PathBuf> {
    if is_default_config_path(config_path, repository_root) {
        return Ok(PathBuf::from(DEFAULT_CONFIG_FILE_NAME));
    }
    if config_path.to_str().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "install cannot generate hook scripts for non-UTF-8 config paths; use a UTF-8 path for --config or GIT_SMEE_CONFIG",
        ));
    }
    Ok(config_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn canonical_inequality_does_not_fall_back_to_lexical_default_match() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        let repository_root = temp_dir.path().join("repo");
        let outside_dir = temp_dir.path().join("outside");
        fs::create_dir_all(&repository_root).expect("failed to create repo");
        fs::create_dir_all(&outside_dir).expect("failed to create outside dir");

        let default_config_path = repository_root.join(DEFAULT_CONFIG_FILE_NAME);
        let outside_config_path = temp_dir.path().join(DEFAULT_CONFIG_FILE_NAME);
        fs::write(&default_config_path, "").expect("failed to write default config");
        fs::write(&outside_config_path, "").expect("failed to write outside config");
        symlink(&outside_dir, repository_root.join("link")).expect("failed to create symlink");

        let config_path = repository_root.join("link/../.git-smee.toml");

        assert!(!is_default_config_path(&config_path, &repository_root));
    }

    #[test]
    fn explicit_relative_config_path_resolves_against_invocation_dir() {
        let invocation_dir = Path::new("/work/repo");

        assert_eq!(
            resolve_config_path(Some(PathBuf::from("custom.toml")), invocation_dir),
            PathBuf::from("/work/repo/custom.toml")
        );
    }

    #[test]
    fn default_hook_script_config_path_stays_repository_relative() {
        let repository_root = Path::new("/work/repo");
        let config_path = repository_root.join(DEFAULT_CONFIG_FILE_NAME);

        assert_eq!(
            normalize_config_path_for_hook_script(&config_path, repository_root)
                .expect("config path should normalize"),
            PathBuf::from(DEFAULT_CONFIG_FILE_NAME)
        );
    }
}
