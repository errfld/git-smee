use std::path::Path;

use git_smee_core::{config::LifeCyclePhase, installer, repository};

pub(crate) fn run_migrate_hooks() -> Result<(), Box<dyn std::error::Error>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_unmanaged_hooks_reports_no_suggestions() {
        let report = MigrationReport::default();

        assert!(
            report
                .to_toml_suggestions()
                .contains("# No unmanaged Git hooks found.")
        );
    }

    #[test]
    fn suggestions_escape_toml_basic_strings() {
        assert_eq!(toml_escape_basic_string(r#"a\b"c"#), r#"a\\b\"c"#);
    }
}
