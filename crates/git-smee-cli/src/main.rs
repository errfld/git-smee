use std::{path::PathBuf, str::FromStr};

use clap::{Parser, command};
use git_smee_core::{config, installer};

#[derive(clap::Parser)]
#[command(name = "git-smee")]
#[command(about = "🏴‍☠️ Smee - the right hand of (Git) hooks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Install,
    Run { hook: String },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let Ok(config_file) = PathBuf::from_str(".git-smee.toml");
    let config = config::SmeeConfig::try_from(config_file.as_path())?;

    match cli.command {
        Command::Install => {
            println!("Installing hooks...");
            let installer = installer::FileSystemHookInstaller::from_default()?;
            installer::install_hooks(&config, &installer)?;
            println!("Hooks installed successfully.");
            Ok(())
        }
        Command::Run { hook } => {
            println!("Running hook: {hook}");
            // Hook execution logic goes here
            Ok(())
        }
    }
}
