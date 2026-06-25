use std::{env, path::Path};

use git_smee_core::{installer, repository};

use crate::config_path::{normalize_config_path_for_hook_script, read_config_file};

pub(crate) fn run_install(
    config_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    repository::ensure_in_repo_root()?;
    let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
    let config_path_for_hooks =
        normalize_config_path_for_hook_script(config_path, &env::current_dir()?)?;
    let hook_script_options =
        installer::HookScriptOptions::new(env::current_exe()?, config_path_for_hooks);
    println!("Installing hooks...");
    let config = read_config_file(config_path)?;
    installer::install_hooks_with_options(&config, &installer, &hook_script_options)?;
    println!("Hooks installed successfully.");
    Ok(())
}
