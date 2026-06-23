use std::{env, fs, path::Path};

use git_smee_core::{installer, installer::MANAGED_FILE_MARKER, repository};
use serde::Serialize;

use crate::{normalize_config_path_for_hook_script, read_config_file};

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

pub(crate) fn run_doctor(config_path: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
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
        if let Some(expected_exe) = &expected_exe {
            let expected_exe = expected_exe.to_string_lossy().to_string();
            if !content.contains(&expected_exe) {
                report.warnings.push(format!(
                    "stale managed wrapper for {phase}: expected executable {expected_exe}; run git smee install"
                ));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_doctor_report_preserves_ok_when_no_findings() {
        let report = finish_doctor_report(DoctorReport {
            status: DoctorStatus::Error,
            repository_root: None,
            hooks_dir: None,
            config_path: ".git-smee.toml".to_string(),
            ok: vec!["inside a Git repository".to_string()],
            warnings: Vec::new(),
            errors: Vec::new(),
        });

        assert!(matches!(report.status, DoctorStatus::Ok));
    }

    #[test]
    fn finish_doctor_report_prefers_errors_over_warnings() {
        let report = finish_doctor_report(DoctorReport {
            status: DoctorStatus::Ok,
            repository_root: None,
            hooks_dir: None,
            config_path: ".git-smee.toml".to_string(),
            ok: Vec::new(),
            warnings: vec!["stale wrapper".to_string()],
            errors: vec!["missing wrapper".to_string()],
        });

        assert!(matches!(report.status, DoctorStatus::Error));
    }
}
