use std::fs;
use tempfile::tempdir;
use xchendev_tools_lib::config_store::{open_at, project_id_from_path};

#[test]
fn project_catalog_persists_and_remove_keeps_project_files() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let project_root = dir.path().join("demo-project");
    fs::create_dir_all(&project_root).unwrap();
    fs::write(project_root.join("keep.txt"), "keep").unwrap();

    let store = open_at(config_path.clone());
    let saved = store
        .upsert_project(project_root.to_str().unwrap(), Some("Demo"))
        .unwrap();
    assert_eq!(saved.name, "Demo");
    assert_eq!(store.list_projects().len(), 1);
    assert!(config_path.is_file());

    drop(store);
    let reloaded = open_at(config_path);
    let projects = reloaded.list_projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_id, saved.project_id);

    assert!(reloaded.remove_project(&saved.project_id).unwrap());
    assert!(reloaded.list_projects().is_empty());
    assert!(project_root.is_dir());
    assert_eq!(
        fs::read_to_string(project_root.join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn project_id_is_stable_for_same_path_case() {
    let a = project_id_from_path(r"C:\\Dev\\Demo");
    let b = project_id_from_path(r"c:\\dev\\demo");
    assert_eq!(a, b);
}

#[test]
fn old_config_without_projects_deserializes_with_empty_catalog() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    fs::write(
        &config_path,
        r#"{
  "version": 1,
  "settings": {},
  "manual_overrides": {},
  "launch_profiles": [],
  "audit_events": [],
  "recent_errors": []
}"#,
    )
    .unwrap();

    let store = open_at(config_path);
    assert!(store.list_projects().is_empty());
}
