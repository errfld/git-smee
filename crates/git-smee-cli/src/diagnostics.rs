use std::{
    env, fs,
    path::{Path, PathBuf},
};

use git_smee_core::{config::LifeCyclePhase, installer};

use crate::config_path::normalize_config_path_for_hook_script;

pub(crate) struct ExpectedHookScript {
    config_path: String,
    executable_path: Option<String>,
}

impl ExpectedHookScript {
    pub(crate) fn from_current_process(config_path: &Path, repository_root: &Path) -> Self {
        let normalized_config_path =
            normalize_config_path_for_hook_script(config_path, repository_root)
                .unwrap_or_else(|_| config_path.to_path_buf());
        Self {
            config_path: normalized_config_path.to_string_lossy().to_string(),
            executable_path: env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().to_string()),
        }
    }

    pub(crate) fn stale_reasons(&self, hook_content: &str) -> Vec<String> {
        let mut reasons = Vec::new();
        if !hook_content.contains(&self.config_path) {
            reasons.push(format!("expected config path {}", self.config_path));
        }
        match &self.executable_path {
            Some(expected_exe) if !hook_content.contains(expected_exe) => {
                reasons.push(format!("expected executable {expected_exe}"));
            }
            _ => {}
        }
        reasons
    }
}

#[derive(Debug)]
pub(crate) struct HookInspection {
    phase: LifeCyclePhase,
    path: PathBuf,
    display_path: String,
    state: HookInspectionState,
}

impl HookInspection {
    pub(crate) fn phase(&self) -> LifeCyclePhase {
        self.phase
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn display_path(&self) -> &str {
        &self.display_path
    }

    pub(crate) fn state(&self) -> &HookInspectionState {
        &self.state
    }
}

#[derive(Debug)]
pub(crate) enum HookInspectionState {
    Missing,
    InvalidPath,
    Unmanaged,
    Managed { content: String },
    Unreadable { error: String },
}

pub(crate) fn inspect_hook(
    repository_root: &Path,
    hooks_dir: &Path,
    phase: LifeCyclePhase,
) -> HookInspection {
    let path = hooks_dir.join(phase.as_str());
    let display_path = display_repo_path(repository_root, &path);
    let state = if !path.exists() {
        HookInspectionState::Missing
    } else if !path.is_file() {
        HookInspectionState::InvalidPath
    } else {
        match installer::has_managed_header(&path) {
            Ok(false) => HookInspectionState::Unmanaged,
            Ok(true) => match fs::read_to_string(&path) {
                Ok(content) => HookInspectionState::Managed { content },
                Err(error) => HookInspectionState::Unreadable {
                    error: error.to_string(),
                },
            },
            Err(error) => HookInspectionState::Unreadable {
                error: error.to_string(),
            },
        }
    };

    HookInspection {
        phase,
        path,
        display_path,
        state,
    }
}

pub(crate) fn inspect_obsolete_managed_hooks(
    repository_root: &Path,
    hooks_dir: &Path,
    configured_phases: &[LifeCyclePhase],
) -> Vec<HookInspection> {
    LifeCyclePhase::all()
        .iter()
        .copied()
        .filter(|phase| !configured_phases.contains(phase))
        .filter_map(|phase| {
            let inspection = inspect_hook(repository_root, hooks_dir, phase);
            matches!(inspection.state(), HookInspectionState::Managed { .. }).then_some(inspection)
        })
        .collect()
}

pub(crate) fn display_repo_path(repository_root: &Path, path: &Path) -> String {
    path.strip_prefix(repository_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use git_smee_core::installer;

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

    #[test]
    fn inspect_hook_treats_marker_only_in_body_as_unmanaged() {
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        let repository_root = temp_dir.path();
        let hooks_dir = repository_root.join(".git/hooks");
        fs::create_dir_all(&hooks_dir).expect("failed to create hooks dir");
        fs::write(
            hooks_dir.join("pre-commit"),
            format!("#!/bin/sh\necho '{}'\n", installer::MANAGED_FILE_MARKER),
        )
        .expect("failed to write hook");

        let inspection = inspect_hook(repository_root, &hooks_dir, LifeCyclePhase::PreCommit);

        assert!(matches!(inspection.state(), HookInspectionState::Unmanaged));
    }

    #[test]
    fn stale_reasons_report_missing_config_and_executable() {
        let expected = ExpectedHookScript {
            config_path: ".git-smee.toml".to_string(),
            executable_path: Some("/bin/git-smee".to_string()),
        };

        assert_eq!(
            expected.stale_reasons("#!/bin/sh\n"),
            vec![
                "expected config path .git-smee.toml".to_string(),
                "expected executable /bin/git-smee".to_string(),
            ]
        );
    }

    #[test]
    fn obsolete_managed_hook_inspection_skips_configured_phases() {
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        let repository_root = temp_dir.path();
        let hooks_dir = repository_root.join(".git/hooks");
        fs::create_dir_all(&hooks_dir).expect("failed to create hooks dir");
        fs::write(
            hooks_dir.join("pre-commit"),
            installer::with_managed_header("#!/bin/sh\n"),
        )
        .expect("failed to write managed hook");
        fs::write(
            hooks_dir.join("pre-push"),
            installer::with_managed_header("#!/bin/sh\n"),
        )
        .expect("failed to write managed hook");

        let inspections = inspect_obsolete_managed_hooks(
            repository_root,
            &hooks_dir,
            &[LifeCyclePhase::PreCommit],
        );

        assert_eq!(
            inspections
                .iter()
                .map(|inspection| inspection.phase().as_str())
                .collect::<Vec<_>>(),
            vec!["pre-push"]
        );
    }
}
