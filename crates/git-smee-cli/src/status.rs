use std::path::Path;

use git_smee_core::repository;
use serde::Serialize;

use crate::{
    config_path::read_config_file,
    diagnostics::{
        ExpectedHookScript, HookInspectionState, inspect_hook, inspect_obsolete_managed_hooks,
    },
};

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
    let hooks_dir = git_smee_core::repository::resolve_git_path(
        &repository_root,
        git_smee_core::installer::FileSystemHookInstaller::HOOKS_GIT_PATH_KEY,
    )?;
    let config = read_config_file(config_path)?;
    let expected_hook_script =
        ExpectedHookScript::from_current_process(config_path, &repository_root);

    let mut phases: Vec<_> = config.hooks.keys().copied().collect();
    phases.sort_by_key(|phase| phase.as_str());

    let mut hooks = Vec::new();
    let mut next_actions = Vec::new();
    for phase in &phases {
        let inspection = inspect_hook(&repository_root, &hooks_dir, *phase);
        let configured_command_count = config.hooks.get(phase).map_or(0, Vec::len);
        let mut stale_reasons = Vec::new();
        let (state, next_action) = match inspection.state() {
            HookInspectionState::Missing => (
                HookState::Missing,
                Some(format!("run git smee install to create {phase}")),
            ),
            HookInspectionState::InvalidPath => (
                HookState::InvalidPath,
                Some(format!(
                    "remove {} or fix core.hooksPath before reinstalling",
                    inspection.display_path()
                )),
            ),
            HookInspectionState::Unmanaged => (
                HookState::Unmanaged,
                Some(format!(
                    "move {} aside or run git smee install --force",
                    inspection.display_path()
                )),
            ),
            HookInspectionState::Managed { content } => {
                stale_reasons = expected_hook_script.stale_reasons(content);
                if stale_reasons.is_empty() {
                    (HookState::Installed, None)
                } else {
                    (
                        HookState::Stale,
                        Some(format!("run git smee install to refresh {phase}")),
                    )
                }
            }
            HookInspectionState::Unreadable { error } => (
                HookState::Unreadable,
                Some(format!(
                    "fix permissions for {} ({error})",
                    inspection.display_path()
                )),
            ),
        };

        if let Some(action) = &next_action {
            next_actions.push(action.clone());
        }
        hooks.push(HookStatus {
            phase: phase.to_string(),
            configured_command_count,
            state,
            path: inspection.display_path().to_string(),
            stale_reasons,
            next_action,
        });
    }

    let mut obsolete_managed_hooks = Vec::new();
    for inspection in inspect_obsolete_managed_hooks(&repository_root, &hooks_dir, &phases) {
        let path = inspection.display_path().to_string();
        let next_action = format!("remove obsolete managed hook {path}");
        next_actions.push(next_action.clone());
        obsolete_managed_hooks.push(ObsoleteHookStatus {
            phase: inspection.phase().to_string(),
            path,
            next_action,
        });
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
