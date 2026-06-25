use std::{env, path::Path};

use clap::ValueEnum;
use git_smee_core::{config, installer, installer::HookInstaller, repository};

use crate::config_path::is_default_config_path;

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum InitTemplate {
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

pub(crate) fn run_init(
    config_path: &Path,
    force: bool,
    template: &InitTemplate,
) -> Result<(), Box<dyn std::error::Error>> {
    repository::ensure_in_repo_root()?;
    let installer = installer::FileSystemHookInstaller::from_default_with_force(force)?;
    println!(
        "Initializing {} configuration file...",
        config_path.display()
    );
    let template_config = template.config_content()?;
    let template_config = installer::with_managed_header(&template_config);

    if is_default_config_path(config_path, &env::current_dir()?) {
        installer.install_config_file(&template_config)?;
    } else {
        installer::write_config_file(config_path, &template_config, force)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_names_match_cli_values() {
        assert_eq!(InitTemplate::Minimal.to_string(), "minimal");
        assert_eq!(InitTemplate::Rust.to_string(), "rust");
        assert_eq!(InitTemplate::NodePnpm.to_string(), "node-pnpm");
        assert_eq!(InitTemplate::Generic.to_string(), "generic");
    }

    #[test]
    fn non_minimal_templates_include_expected_hooks() {
        let rust = InitTemplate::Rust.config_content().expect("rust template");
        assert!(rust.contains("[[pre-commit]]"));
        assert!(rust.contains("[[pre-push]]"));

        let node = InitTemplate::NodePnpm
            .config_content()
            .expect("node template");
        assert!(node.contains("pnpm lint"));
        assert!(node.contains("pnpm test"));
    }
}
