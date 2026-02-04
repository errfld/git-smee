use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use clap::Parser;
use git_smee_core::{
    DEFAULT_CONFIG_FILE_NAME, SmeeConfig, config, executor,
    installer::{self, HookScriptOptions},
    repository,
};

#[derive(clap::Parser)]
#[command(name = "git-smee")]
#[command(about = "🏴‍☠️ Smee - the right hand of (Git) hooks", long_about = None)]
#[command(version)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    #[command(
        name = "install",
        about = "Install git hooks from {.git-smee.toml} into Git's effective hooks directory"
    )]
    Install {
        #[arg(long, help = "Overwrite existing unmanaged hook files")]
        force: bool,
    },
    #[command(name = "run", about = "Run a specific git hook")]
    Run { hook: String },
    #[command(
        name = "init",
        about = "Initialize a .git-smee.toml configuration file"
    )]
    Initialize {
        #[arg(long, help = "Overwrite an existing .git-smee.toml file")]
        force: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config);

    match cli.command {
        Command::Install { force } => {
            repository::ensure_in_repo_root()?;
            let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
            let config_path_for_hooks = normalize_config_path_for_hook_script(&config_path);
            let hook_script_options =
                HookScriptOptions::new(env::current_exe()?, config_path_for_hooks);
            println!("Installing hooks...");
            let config = read_config_file(&config_path)?;
            installer::install_hooks_with_options(&config, &installer, &hook_script_options)?;
            println!("Hooks installed successfully.");
            Ok(())
        }
        Command::Run { hook } => {
            repository::ensure_in_repo_root()?;
            println!("Running hook: {hook}");
            let config = read_config_file(&config_path)?;
            let phase = config::LifeCyclePhase::from_str(&hook)?;
            executor::execute_hook(&config, phase).map_err(Box::from)
        }
        Command::Initialize { force } => {
            repository::ensure_in_repo_root()?;
            let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
            println!(
                "Initializing {} configuration file...",
                config_path.display()
            );
            let default_config: String = (&config::SmeeConfig::default()).try_into()?;
            let default_config = installer::with_managed_header(&default_config);

            if is_default_config_path(&config_path) {
                installer.install_config_file(&default_config)?;
            } else {
                write_config_file(&config_path, &default_config, force)?;
            }
            Ok(())
        }
    }
}

fn resolve_config_path(cli_config: Option<PathBuf>) -> PathBuf {
    if let Some(path) = cli_config {
        return path;
    }
    if let Ok(path_from_env) = env::var("GIT_SMEE_CONFIG") {
        if !path_from_env.trim().is_empty() {
            return PathBuf::from(path_from_env);
        }
    }
    PathBuf::from_str(DEFAULT_CONFIG_FILE_NAME).expect("default config path should be valid")
}

fn read_config_file(config_path: &Path) -> Result<SmeeConfig, config::Error> {
    config::SmeeConfig::try_from(config_path)
}

fn write_config_file(config_path: &Path, content: &str, force: bool) -> Result<(), std::io::Error> {
    if config_path.exists() && !force {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "Refusing to overwrite existing config file '{}'. Re-run with --force to overwrite.",
                config_path.display()
            ),
        ));
    }
    if let Some(parent) = config_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(config_path, content)
}

fn is_default_config_path(config_path: &Path) -> bool {
    config_path == Path::new(DEFAULT_CONFIG_FILE_NAME)
}

fn normalize_config_path_for_hook_script(config_path: &Path) -> PathBuf {
    if config_path.is_absolute() {
        return config_path.to_path_buf();
    }

    env::current_dir()
        .map(|cwd| cwd.join(config_path))
        .unwrap_or_else(|_| config_path.to_path_buf())
}
