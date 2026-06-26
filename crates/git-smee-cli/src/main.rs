use std::path::PathBuf;

use clap::Parser;

mod commands;
mod config_path;
mod diagnostics;
mod doctor;
mod status;

use commands::init::InitTemplate;
use config_path::resolve_config_path;

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
    Run {
        hook: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        hook_args: Vec<String>,
    },
    #[command(
        name = "init",
        about = "Initialize a .git-smee.toml configuration file"
    )]
    Initialize {
        #[arg(long, help = "Overwrite an existing .git-smee.toml file")]
        force: bool,
        #[arg(
            long,
            default_value_t = InitTemplate::Minimal,
            value_enum,
            help = "Starter template to write"
        )]
        template: InitTemplate,
    },
    #[command(name = "doctor", about = "Diagnose git-smee repository setup")]
    Doctor {
        #[arg(long, help = "Emit a stable JSON diagnostics report")]
        json: bool,
    },
    #[command(name = "status", about = "Show installed hook coverage and drift")]
    Status {
        #[arg(long, help = "Emit a stable JSON status report")]
        json: bool,
    },
    #[command(
        name = "migrate-hooks",
        about = "Suggest git-smee config entries for existing unmanaged Git hooks"
    )]
    MigrateHooks,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let invocation_dir = std::env::current_dir()?;
    let config_path = resolve_config_path(cli.config, &invocation_dir);

    match cli.command {
        Command::Install { force } => commands::install::run_install(&config_path, force),
        Command::Run { hook, hook_args } => {
            commands::run::run_hook(&config_path, &hook, &hook_args)
        }
        Command::Initialize { force, template } => {
            commands::init::run_init(&config_path, force, &template)
        }
        Command::Doctor { json } => doctor::run_doctor(&config_path, json),
        Command::Status { json } => status::run_status(&config_path, json),
        Command::MigrateHooks => commands::migrate_hooks::run_migrate_hooks(),
    }
}
