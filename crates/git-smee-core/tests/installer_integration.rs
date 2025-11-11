use std::fs;

use git_smee_core::{
    SmeeConfig,
    installer::{self, FileSystemHookInstaller},
};

#[test]
fn given_simple_config_when_installing_hooks_then_no_error() {
    // given
    let config_content = fs::read_to_string("tests/fixtures/simple_git-smee_config.toml")
        .expect("Should read fixture file");

    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("smee.toml");
    std::fs::write(&config_path, config_content).unwrap();

    let config: SmeeConfig = config_path.as_path().try_into().unwrap();
    let installer = FileSystemHookInstaller::from_path(temp_dir.path().to_path_buf()).unwrap();
    // when
    let result = installer::install_hooks(&config, &installer);

    // then
    assert!(result.is_ok());
    let hook = temp_dir.path().join("pre-commit");
    assert!(hook.exists());
    assert!(
        fs::read_to_string(&hook)
            .unwrap()
            .contains("git smee run pre-commit")
    );
    let hook = temp_dir.path().join("pre-push");
    assert!(hook.exists());
    assert!(
        fs::read_to_string(&hook)
            .unwrap()
            .contains("git smee run pre-push")
    );
}
