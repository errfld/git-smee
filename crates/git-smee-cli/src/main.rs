use std::path::{Path, PathBuf};

use clap::Parser;
use git_smee_core::{config::LifeCyclePhase, installer, repository};

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
        Command::MigrateHooks => run_migrate_hooks(),
    }
}

fn run_migrate_hooks() -> Result<(), Box<dyn std::error::Error>> {
    repository::ensure_in_repo_root()?;
    let installer = installer::FileSystemHookInstaller::from_default()?;
    let report = MigrationReport::from_hooks_dir(installer.effective_hooks_dir())?;

    println!("{}", report.to_toml_suggestions());
    Ok(())
}

#[derive(Debug, Default)]
struct MigrationReport {
    unmanaged_hooks: Vec<LifeCyclePhase>,
    managed_hooks: Vec<LifeCyclePhase>,
}

impl MigrationReport {
    fn from_hooks_dir(hooks_dir: &Path) -> Result<Self, installer::Error> {
        let mut report = Self::default();

        for phase in LifeCyclePhase::all() {
            let hook_path = hooks_dir.join(phase.as_str());
            if !hook_path.is_file() {
                continue;
            }

            if installer::has_managed_header(&hook_path)? {
                report.managed_hooks.push(*phase);
            } else {
                report.unmanaged_hooks.push(*phase);
            }
        }

        Ok(report)
    }

    fn to_toml_suggestions(&self) -> String {
        let mut lines = vec![
            "# git-smee hook migration suggestions".to_string(),
            "# Review these commands before adding them to .git-smee.toml.".to_string(),
            "# Dry-run only: this command does not modify hook files or configuration.".to_string(),
        ];

        if !self.managed_hooks.is_empty() {
            lines.push(format!(
                "# Ignored managed git-smee hooks: {}",
                join_phase_names(&self.managed_hooks)
            ));
        }

        if self.unmanaged_hooks.is_empty() {
            lines.push("# No unmanaged Git hooks found.".to_string());
            return finish_lines(lines);
        }

        lines.push(String::new());
        for phase in &self.unmanaged_hooks {
            lines.push(format!("[[{}]]", phase.as_str()));
            lines.push(format!(
                "command = \"{}\"",
                toml_escape_basic_string(&format!(".git-smee/legacy/{}", phase.as_str()))
            ));
            lines.push(format!(
                "# TODO: move .git/hooks/{0} to .git-smee/legacy/{0} before running git smee install.",
                phase.as_str()
            ));
            lines.push(String::new());
        }

        finish_lines(lines)
    }
}

fn join_phase_names(phases: &[LifeCyclePhase]) -> String {
    phases
        .iter()
        .map(|phase| phase.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn toml_escape_basic_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn finish_lines(lines: Vec<String>) -> String {
    let mut output = lines.join("\n");
    output.push('\n');
    output
}
