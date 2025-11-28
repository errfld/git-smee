use std::{path::PathBuf, str::FromStr};

use clap::{Parser, command};
use git_smee_core::{
    DEFAULT_CONFIG_FILE_NAME, SmeeConfig, config, executor,
    installer::{self, HookInstaller},
    repository,
};

#[derive(clap::Parser)]
#[command(name = "git-smee")]
#[command(about = "🏴‍☠️ Smee - the right hand of (Git) hooks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    #[command(
        name = "install",
        about = "Install git hooks from {.git-smee.toml} into .git/hooks"
    )]
    Install,
    #[command(name = "run", about = "Run a specific git hook")]
    Run { hook: String },
    #[command(
        name = "init",
        about = "Initialize a .git-smee.toml configuration file"
    )]
    Initialize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure we're in a git repository and navigate to the root
    repository::ensure_in_repo_root()?;

    let cli = Cli::parse();

    let installer = installer::FileSystemHookInstaller::from_default()?;

    match cli.command {
        Command::Install => {
            println!("Installing hooks...");
            let config = read_config_file()?;
            installer::install_hooks(&config, &installer)?;
            println!("Hooks installed successfully.");
            Ok(())
        }
        Command::Run { hook } => {
            println!("Running hook: {hook}");
            let config = read_config_file()?;
            let phase = config::LifeCyclePhase::from_str(&hook)?;
            executor::execute_hook(&config, phase).map_err(Box::from)
        }
        Command::Initialize => {
            println!("Initializing {DEFAULT_CONFIG_FILE_NAME} configuration file...");
            let default_config: String = (&config::SmeeConfig::default()).try_into()?;
            installer.install_config_file(&default_config)?;
            Ok(())
        }
    }
}

fn read_config_file() -> Result<SmeeConfig, config::Error> {
    let Ok(config_file) = PathBuf::from_str(DEFAULT_CONFIG_FILE_NAME);
    config::SmeeConfig::try_from(config_file.as_path())
}
