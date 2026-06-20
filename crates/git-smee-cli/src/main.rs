use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, IsTerminal, Read},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use clap::Parser;
use git_smee_core::{
    DEFAULT_CONFIG_FILE_NAME, SmeeConfig,
    config::{self, LifeCyclePhase},
    executor,
    installer::{self, HookInstaller, HookScriptOptions, MANAGED_FILE_MARKER},
    repository,
};
use serde::Serialize;

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
}

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
        Command::Initialize { force } => {
            repository::ensure_in_repo_root()?;
            let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
            println!(
                "Initializing {} configuration file...",
                config_path.display()
            );
            let default_config: String = (&config::SmeeConfig::default()).try_into()?;
            let default_config = installer::with_managed_header(&default_config);

            if is_default_config_path(&config_path, &env::current_dir()?) {
                installer.install_config_file(&default_config)?;
            } else {
                installer::write_config_file(&config_path, &default_config, force)?;
            }
            Ok(())
        }
        Command::Doctor { json } => run_doctor(&config_path, json),
        Command::Status { json } => run_status(&config_path, json),
    }
}

#[derive(Debug, Serialize)]
struct StatusReport {
    status: StatusState,
    repository_root: String,
    hooks_dir: String,
    config_path: String,
    hooks: Vec<HookStatus>,
    obsolete_managed_hooks: Vec<ObsoleteHookStatus>,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum StatusState {
    Ok,
    Drift,
}

#[derive(Debug, Serialize)]
struct HookStatus {
    phase: String,
    configured_command_count: usize,
    state: HookState,
    path: String,
    stale_reasons: Vec<String>,
    next_action: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum HookState {
    Installed,
    Missing,
    Unmanaged,
    Stale,
    Unreadable,
    InvalidPath,
}

#[derive(Debug, Serialize)]
struct ObsoleteHookStatus {
    phase: String,
    path: String,
    next_action: String,
}

fn run_status(config_path: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    repository::ensure_in_repo_root()?;
    let report = build_status_report(config_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_status_report(&report);
    }
    Ok(())
}

fn build_status_report(config_path: &Path) -> Result<StatusReport, Box<dyn std::error::Error>> {
    let repository_root = repository::find_git_root()?;
    let hooks_dir = repository::resolve_git_path(
        &repository_root,
        installer::FileSystemHookInstaller::HOOKS_GIT_PATH_KEY,
    )?;
    let config = read_config_file(config_path)?;
    let expected_config_path = normalize_config_path_for_hook_script(config_path, &repository_root)
        .unwrap_or_else(|_| config_path.to_path_buf());
    let expected_exe = env::current_exe().ok();
    let expected_config = expected_config_path.to_string_lossy().to_string();

    let mut phases: Vec<_> = config.hooks.keys().copied().collect();
    phases.sort_by_key(|phase| phase.as_str());

    let mut hooks = Vec::new();
    let mut next_actions = Vec::new();
    for phase in &phases {
        let hook_path = hooks_dir.join(phase.as_str());
        let configured_command_count = config.hooks.get(phase).map_or(0, Vec::len);
        let mut stale_reasons = Vec::new();
        let (state, next_action) = if !hook_path.exists() {
            (
                HookState::Missing,
                Some(format!("run git smee install to create {phase}")),
            )
        } else if !hook_path.is_file() {
            (
                HookState::InvalidPath,
                Some(format!(
                    "remove {} or fix core.hooksPath before reinstalling",
                    display_repo_path(&repository_root, &hook_path)
                )),
            )
        } else {
            match fs::read_to_string(&hook_path) {
                Ok(content) if !content.contains(MANAGED_FILE_MARKER) => (
                    HookState::Unmanaged,
                    Some(format!(
                        "move {} aside or run git smee install --force",
                        display_repo_path(&repository_root, &hook_path)
                    )),
                ),
                Ok(content) => {
                    if !content.contains(&expected_config) {
                        stale_reasons.push(format!("expected config path {expected_config}"));
                    }
                    if let Some(expected_exe) = &expected_exe {
                        let expected_exe = expected_exe.to_string_lossy().to_string();
                        if !content.contains(&expected_exe) {
                            stale_reasons.push(format!("expected executable {expected_exe}"));
                        }
                    }
                    if stale_reasons.is_empty() {
                        (HookState::Installed, None)
                    } else {
                        (
                            HookState::Stale,
                            Some(format!("run git smee install to refresh {phase}")),
                        )
                    }
                }
                Err(error) => (
                    HookState::Unreadable,
                    Some(format!(
                        "fix permissions for {} ({error})",
                        display_repo_path(&repository_root, &hook_path)
                    )),
                ),
            }
        };

        if let Some(action) = &next_action {
            next_actions.push(action.clone());
        }
        hooks.push(HookStatus {
            phase: phase.to_string(),
            configured_command_count,
            state,
            path: display_repo_path(&repository_root, &hook_path),
            stale_reasons,
            next_action,
        });
    }

    let configured_phase_names: Vec<_> = phases.iter().map(|phase| phase.as_str()).collect();
    let mut obsolete_managed_hooks = Vec::new();
    for phase in LifeCyclePhase::all() {
        if configured_phase_names.contains(&phase.as_str()) {
            continue;
        }
        let hook_path = hooks_dir.join(phase.as_str());
        if !hook_path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&hook_path) else {
            continue;
        };
        if content.contains(MANAGED_FILE_MARKER) {
            let path = display_repo_path(&repository_root, &hook_path);
            let next_action = format!("remove obsolete managed hook {path}");
            next_actions.push(next_action.clone());
            obsolete_managed_hooks.push(ObsoleteHookStatus {
                phase: phase.to_string(),
                path,
                next_action,
            });
        }
    }

    next_actions.sort();
    next_actions.dedup();

    let status = if next_actions.is_empty() {
        StatusState::Ok
    } else {
        StatusState::Drift
    };

    Ok(StatusReport {
        status,
        repository_root: repository_root.display().to_string(),
        hooks_dir: hooks_dir.display().to_string(),
        config_path: config_path.display().to_string(),
        hooks,
        obsolete_managed_hooks,
        next_actions,
    })
}

fn print_status_report(report: &StatusReport) {
    println!("git-smee status: {:?}", report.status);
    println!("repository root: {}", report.repository_root);
    println!("hooks directory: {}", report.hooks_dir);
    println!("config path: {}", report.config_path);
    println!("configured hooks:");
    if report.hooks.is_empty() {
        println!("  - none");
    } else {
        for hook in &report.hooks {
            println!(
                "  - {}: configured commands={}, {} ({})",
                hook.phase,
                hook.configured_command_count,
                hook.state.as_text(),
                hook.path
            );
            for reason in &hook.stale_reasons {
                println!("    stale: {reason}");
            }
            if let Some(action) = &hook.next_action {
                println!("    next: {action}");
            }
        }
    }
    println!("obsolete managed hooks:");
    if report.obsolete_managed_hooks.is_empty() {
        println!("  - none");
    } else {
        for hook in &report.obsolete_managed_hooks {
            println!(
                "  - {}: obsolete managed wrapper ({})",
                hook.phase, hook.path
            );
            println!("    next: {}", hook.next_action);
        }
    }
    print_doctor_section("next actions", &report.next_actions);
}

impl HookState {
    fn as_text(&self) -> &'static str {
        match self {
            HookState::Installed => "installed",
            HookState::Missing => "missing",
            HookState::Unmanaged => "unmanaged",
            HookState::Stale => "stale",
            HookState::Unreadable => "unreadable",
            HookState::InvalidPath => "invalid path",
        }
    }
}

fn display_repo_path(repository_root: &Path, path: &Path) -> String {
    path.strip_prefix(repository_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    status: DoctorStatus,
    repository_root: Option<String>,
    hooks_dir: Option<String>,
    config_path: String,
    ok: Vec<String>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum DoctorStatus {
    Ok,
    Warning,
    Error,
}

fn run_doctor(config_path: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let report = build_doctor_report(config_path);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_doctor_report(&report);
    }
    if report.errors.is_empty() {
        Ok(())
    } else {
        Err("doctor found repository setup errors".into())
    }
}

fn build_doctor_report(config_path: &Path) -> DoctorReport {
    let mut report = DoctorReport {
        status: DoctorStatus::Ok,
        repository_root: None,
        hooks_dir: None,
        config_path: config_path.display().to_string(),
        ok: Vec::new(),
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let repository_root = match repository::find_git_root() {
        Ok(root) => {
            report.ok.push("inside a Git repository".to_string());
            report.repository_root = Some(root.display().to_string());
            root
        }
        Err(error) => {
            report.errors.push(format!(
                "not inside a Git repository; run git smee doctor from a repository ({error})"
            ));
            return finish_doctor_report(report);
        }
    };

    let hooks_dir = match repository::resolve_git_path(
        &repository_root,
        installer::FileSystemHookInstaller::HOOKS_GIT_PATH_KEY,
    ) {
        Ok(path) => {
            report.hooks_dir = Some(path.display().to_string());
            if path.exists() && path.is_dir() {
                report
                    .ok
                    .push(format!("hooks directory exists at {}", path.display()));
            } else if path.exists() {
                report.errors.push(format!(
                    "effective hooks path is not a directory: {}; fix core.hooksPath or remove the file",
                    path.display()
                ));
            } else {
                report.warnings.push(format!(
                    "hooks directory does not exist yet at {}; run git smee install to create it",
                    path.display()
                ));
            }
            path
        }
        Err(error) => {
            report.errors.push(format!(
                "could not resolve effective hooks directory; check git core.hooksPath ({error})"
            ));
            return finish_doctor_report(report);
        }
    };

    let config = match read_config_file(config_path) {
        Ok(config) => {
            report
                .ok
                .push(format!("config parses from {}", config_path.display()));
            if config.hooks.is_empty() {
                report.errors.push(
                    "configuration contains no hooks; add at least one [[hook-name]] entry"
                        .to_string(),
                );
            } else {
                report.ok.push(format!(
                    "{} configured hook phase(s) are valid",
                    config.hooks.len()
                ));
            }
            config
        }
        Err(error) => {
            report.errors.push(format!(
                "config problem at {}: {error}; run git smee init or fix the TOML file",
                config_path.display()
            ));
            return finish_doctor_report(report);
        }
    };

    let expected_config_path = normalize_config_path_for_hook_script(config_path, &repository_root)
        .unwrap_or_else(|_| config_path.to_path_buf());
    let expected_exe = env::current_exe().ok();
    let expected_config = expected_config_path.to_string_lossy().to_string();

    let mut phases: Vec<_> = config.hooks.keys().copied().collect();
    phases.sort_by_key(|phase| phase.as_str());
    for phase in phases {
        let hook_path = hooks_dir.join(phase.as_str());
        if !hook_path.exists() {
            report.errors.push(format!(
                "missing managed wrapper for {phase} at {}; run git smee install",
                hook_path.display()
            ));
            continue;
        }
        if !hook_path.is_file() {
            report.errors.push(format!(
                "hook path for {phase} is not a regular file: {}; remove it or fix core.hooksPath",
                hook_path.display()
            ));
            continue;
        }
        let content = match fs::read_to_string(&hook_path) {
            Ok(content) => content,
            Err(error) => {
                report.errors.push(format!(
                    "cannot read hook wrapper for {phase} at {}: {error}",
                    hook_path.display()
                ));
                continue;
            }
        };
        if !content.contains(MANAGED_FILE_MARKER) {
            report.errors.push(format!(
                "unmanaged hook file blocks install for {phase} at {}; move it aside or run git smee install --force",
                hook_path.display()
            ));
            continue;
        }
        report
            .ok
            .push(format!("managed wrapper is installed for {phase}"));
        if !content.contains(&expected_config) {
            report.warnings.push(format!(
                "stale managed wrapper for {phase}: expected config path {}; run git smee install",
                expected_config
            ));
        }
        if let Some(expected_exe) = &expected_exe
            && !content.contains(&expected_exe.to_string_lossy().to_string())
        {
            report.warnings.push(format!(
                "stale managed wrapper for {phase}: expected executable {}; run git smee install",
                expected_exe.display()
            ));
        }
    }

    finish_doctor_report(report)
}

fn finish_doctor_report(mut report: DoctorReport) -> DoctorReport {
    report.status = if !report.errors.is_empty() {
        DoctorStatus::Error
    } else if !report.warnings.is_empty() {
        DoctorStatus::Warning
    } else {
        DoctorStatus::Ok
    };
    report
}

fn print_doctor_report(report: &DoctorReport) {
    println!("git-smee doctor: {:?}", report.status);
    if let Some(root) = &report.repository_root {
        println!("repository root: {root}");
    }
    if let Some(hooks_dir) = &report.hooks_dir {
        println!("hooks directory: {hooks_dir}");
    }
    println!("config path: {}", report.config_path);
    print_doctor_section("ok", &report.ok);
    print_doctor_section("warnings", &report.warnings);
    print_doctor_section("errors", &report.errors);
}

fn print_doctor_section(name: &str, items: &[String]) {
    println!("{name}:");
    if items.is_empty() {
        println!("  - none");
    } else {
        for item in items {
            println!("  - {item}");
        }
    }
}

fn resolve_config_path(cli_config: Option<PathBuf>, invocation_dir: &Path) -> PathBuf {
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

fn read_config_file(config_path: &Path) -> Result<SmeeConfig, config::Error> {
    config::SmeeConfig::try_from(config_path)
}

fn is_default_config_path(config_path: &Path, repository_root: &Path) -> bool {
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

fn normalize_config_path_for_hook_script(
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
}
