use std::{env, fs, path::Path};

use git_smee_core::{config::LifeCyclePhase, installer, repository};
use serde::Serialize;

use crate::config_path::{normalize_config_path_for_hook_script, read_config_file};

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

pub(crate) fn run_status(config_path: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
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
            match installer::has_managed_header(&hook_path) {
                Ok(false) => (
                    HookState::Unmanaged,
                    Some(format!(
                        "move {} aside or run git smee install --force",
                        display_repo_path(&repository_root, &hook_path)
                    )),
                ),
                Ok(true) => match fs::read_to_string(&hook_path) {
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
                },
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
        let Ok(is_managed) = installer::has_managed_header(&hook_path) else {
            continue;
        };
        if is_managed {
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
    println!("git-smee status: {}", report.status.as_text());
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
    print_status_section("next actions", &report.next_actions);
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

impl StatusState {
    fn as_text(&self) -> &'static str {
        match self {
            StatusState::Ok => "Ok",
            StatusState::Drift => "Drift",
        }
    }
}

fn print_status_section(name: &str, items: &[String]) {
    println!("{name}:");
    if items.is_empty() {
        println!("  - none");
    } else {
        for item in items {
            println!("  - {item}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_repo_path_uses_forward_slashes_for_repo_relative_paths() {
        assert_eq!(
            display_repo_path(Path::new("/repo"), Path::new("/repo/.git/hooks/pre-commit")),
            ".git/hooks/pre-commit"
        );
    }

    #[test]
    fn display_repo_path_keeps_external_paths_visible() {
        assert_eq!(
            display_repo_path(Path::new("/repo"), Path::new("/tmp/hooks/pre-commit")),
            "/tmp/hooks/pre-commit"
        );
    }
}
