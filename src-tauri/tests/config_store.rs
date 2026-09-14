use std::path::PathBuf;
use xchendev_tools_lib::config_store::{config_dir, config_file_path};

#[test]
fn config_paths_use_config_subdirectory() {
    let file = config_file_path();
    assert!(
        file.ends_with(PathBuf::from("config").join("config.json")),
        "expected config/config.json, got {}",
        file.display()
    );
    let dir = config_dir();
    assert!(
        dir.ends_with("config"),
        "expected */config, got {}",
        dir.display()
    );
}
