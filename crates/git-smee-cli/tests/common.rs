use std::{fs, path::PathBuf};

use assert_fs::{TempDir, assert::PathAssert, fixture::ChildPath, prelude::PathChild};
use git_smee_core::{DEFAULT_CONFIG_FILE_NAME, config::LifeCyclePhase};
use predicates::path::{exists, is_file};

pub struct TestRepo {
    pub path: TempDir,
}

impl TestRepo {
    pub fn new() -> Self {
        let path = TempDir::new().expect("Failed to create temp dir");
        // Initialize a new git repository
        let _repo = git2::Repository::init(&path).expect("Failed to initialize git repository");

        let test_repo = TestRepo { path };
        test_repo.create_config();
        test_repo
    }

    const CONFIG_CONTENTS: &str = r#"
    [[pre-commit]]
    command = "echo Pre-commit hook executed"

    [[pre-push]]
    command = "echo Pre-push hook executed"
    "#;

    pub fn config_path(&self) -> PathBuf {
        self.path.join(DEFAULT_CONFIG_FILE_NAME)
    }

    pub fn create_config(&self) {
        self.write_config(Self::CONFIG_CONTENTS);
    }

    pub fn write_config(&self, contents: &str) {
        fs::write(self.config_path(), contents).expect("Unable to write test config");
    }

    pub fn write_config_at(&self, relative_path: &str, contents: &str) -> PathBuf {
        let path = self.path.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("Unable to create custom config directory");
        }
        fs::write(&path, contents).expect("Unable to write custom test config");
        path
    }

    pub fn hooks_path(&self) -> ChildPath {
        self.path.child(".git").child("hooks")
    }

    pub fn assert_hooks_installed(&self, phases: Vec<LifeCyclePhase>) {
        phases.iter().for_each(|phase| {
            let hook_file = self.hooks_path().child(phase.to_string());
            hook_file.assert(exists());
            hook_file.assert(is_file());
        });
    }
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}
