use std::fs;

use git_smee_core::{
    DEFAULT_CONFIG_FILE_NAME, SmeeConfig,
    installer::{self, FileSystemHookInstaller},
};

#[test]
fn given_simple_config_when_installing_hooks_then_no_error() {
    // given
    let config_content = fs::read_to_string("tests/fixtures/simple_git-smee_config.toml")
        .expect("Should read fixture file");

    let temp_dir = tempfile::tempdir().unwrap();
    let hooks_dir = temp_dir.path().join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).expect("Could not create temporary hoods directory .git/hooks");

    let config_path = temp_dir.path().join(DEFAULT_CONFIG_FILE_NAME);
    assert!(std::fs::write(&config_path, config_content).is_ok());

    let config: SmeeConfig = config_path
        .as_path()
        .try_into()
        .expect("Not able to read smee config from config_path");
    let installer = FileSystemHookInstaller::from_path(temp_dir.path().to_path_buf())
        .expect("No able to create Filesystem installer in temp dir");
    // when
    let result = installer::install_hooks(&config, &installer);

    // then
    assert!(result.is_ok());
    let hook = hooks_dir.join("pre-commit");
    assert!(hook.exists());
    assert!(
        fs::read_to_string(&hook)
            .unwrap()
            .contains("git smee run pre-commit")
    );
    let hook = hooks_dir.join("pre-push");
    assert!(hook.exists());
    assert!(
        fs::read_to_string(&hook)
            .unwrap()
            .contains("git smee run pre-push")
    );
    assert!(!hooks_dir.join("unknown").exists());
}
