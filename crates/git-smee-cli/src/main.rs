use std::{
    env,
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
    str::FromStr,
};

use clap::{Parser, ValueEnum};
use git_smee_core::{
    config::{self, LifeCyclePhase},
    executor,
    installer::{self, HookInstaller, HookScriptOptions},
    repository,
};

mod config_path;
mod diagnostics;
mod doctor;
mod status;

use config_path::{
    is_default_config_path, normalize_config_path_for_hook_script, read_config_file,
    resolve_config_path,
};

const DEFAULT_MAX_HOOK_STDIN_BYTES: u64 = 10 * 1024 * 1024;
const HOOK_STDIN_LIMIT_ENV: &str = "GIT_SMEE_HOOK_STDIN_LIMIT_BYTES";
const DEFAULT_HOOK_STDIN_LIMIT_DISPLAY: &str = "10 MiB";

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

#[derive(Clone, Debug, ValueEnum)]
enum InitTemplate {
    Minimal,
    Rust,
    NodePnpm,
    Generic,
}

impl std::fmt::Display for InitTemplate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Minimal => "minimal",
            Self::Rust => "rust",
            Self::NodePnpm => "node-pnpm",
            Self::Generic => "generic",
        };
        formatter.write_str(name)
    }
}

impl InitTemplate {
    fn config_content(&self) -> Result<String, config::Error> {
        match self {
            Self::Minimal => (&config::SmeeConfig::default()).try_into(),
            Self::Rust => Ok(RUST_INIT_TEMPLATE.to_string()),
            Self::NodePnpm => Ok(NODE_PNPM_INIT_TEMPLATE.to_string()),
            Self::Generic => Ok(GENERIC_INIT_TEMPLATE.to_string()),
        }
    }
}

const RUST_INIT_TEMPLATE: &str = r#"# Rust starter: edit commands to match your workspace policy.
[[pre-commit]]
command = "cargo fmt --all -- --check"

[[pre-commit]]
command = "cargo clippy --workspace --all-targets --all-features -- -D warnings"

[[pre-push]]
command = "cargo test --workspace --all-targets --all-features"
"#;

const NODE_PNPM_INIT_TEMPLATE: &str = r#"# Node/pnpm starter: commands are explicit and editable.
[[pre-commit]]
command = "pnpm lint"

[[pre-push]]
command = "pnpm test"
"#;

const GENERIC_INIT_TEMPLATE: &str = r#"# Generic starter: replace these commands with your project's checks.
# Add another [[pre-commit]] or [[pre-push]] table for each command to run.
[[pre-commit]]
command = "echo 'replace me with your pre-commit check'"

# Example:
# [[pre-push]]
# command = "./scripts/test"
"#;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let invocation_dir = env::current_dir()?;
    let config_path = resolve_config_path(cli.config, &invocation_dir);

    match cli.command {
        Command::Install { force } => {
            repository::ensure_in_repo_root()?;
            let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
            let config_path_for_hooks =
                normalize_config_path_for_hook_script(&config_path, &env::current_dir()?)?;
            let hook_script_options =
                HookScriptOptions::new(env::current_exe()?, config_path_for_hooks);
            println!("Installing hooks...");
            let config = read_config_file(&config_path)?;
            installer::install_hooks_with_options(&config, &installer, &hook_script_options)?;
            println!("Hooks installed successfully.");
            Ok(())
        }
        Command::Run { hook, hook_args } => {
            repository::ensure_in_repo_root()?;
            let phase = LifeCyclePhase::from_str(&hook)?;
            let stdin_payload = read_hook_stdin_for_phase(phase)?;
            let config = read_config_file(&config_path)?;
            let summary = executor::execute_hook_with_summary(
                &config,
                phase,
                &hook_args,
                stdin_payload.as_deref(),
            )?;
            for line in summary.text_lines(phase) {
                println!("{line}");
            }
            if let Some(error) = summary.error() {
                return Err(Box::new(error));
            }
            Ok(())
        }
        Command::Initialize { force, template } => {
            repository::ensure_in_repo_root()?;
            let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
            println!(
                "Initializing {} configuration file...",
                config_path.display()
            );
            let template_config = template.config_content()?;
            let template_config = installer::with_managed_header(&template_config);

            if is_default_config_path(&config_path, &env::current_dir()?) {
                installer.install_config_file(&template_config)?;
            } else {
                installer::write_config_file(&config_path, &template_config, force)?;
            }
            Ok(())
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

fn read_hook_stdin_for_phase(phase: LifeCyclePhase) -> io::Result<Option<Vec<u8>>> {
    // proc-receive is an interactive pkt-line protocol: Git waits for the hook to
    // answer before closing stdin, so buffering until EOF would deadlock before
    // the configured command is spawned. Let the command inherit stdin instead.
    if phase == LifeCyclePhase::ProcReceive {
        return Ok(None);
    }
    read_hook_stdin()
}

fn read_hook_stdin() -> io::Result<Option<Vec<u8>>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let max_hook_stdin_bytes = max_hook_stdin_bytes()?;
    let mut payload = Vec::new();
    stdin
        .lock()
        .take(max_hook_stdin_bytes + 1)
        .read_to_end(&mut payload)?;
    if payload.len() as u64 > max_hook_stdin_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "hook stdin exceeds the {} limit",
                hook_stdin_limit_display(max_hook_stdin_bytes)
            ),
        ));
    }
    Ok(Some(payload))
}

fn max_hook_stdin_bytes() -> io::Result<u64> {
    let Some(value) = env::var_os(HOOK_STDIN_LIMIT_ENV) else {
        return Ok(DEFAULT_MAX_HOOK_STDIN_BYTES);
    };
    let value = value.to_string_lossy();
    value.parse::<u64>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{HOOK_STDIN_LIMIT_ENV} must be an unsigned byte count: {error}"),
        )
    })
}

fn hook_stdin_limit_display(limit: u64) -> String {
    if limit == DEFAULT_MAX_HOOK_STDIN_BYTES {
        DEFAULT_HOOK_STDIN_LIMIT_DISPLAY.to_string()
    } else {
        format!("{limit} bytes")
    }
}
