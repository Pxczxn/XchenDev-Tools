use std::fs;
use tempfile::tempdir;

#[test]
fn scan_finds_package_json_at_root() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
    let result = xchendev_tools_lib::project_scanner::scan_project_directory(
        dir.path().to_str().unwrap(),
    )
    .unwrap();
    assert!(!result.candidates.is_empty());
    assert_eq!(result.candidates[0].stack, xchendev_tools_lib::domain::TechnologyStack::Node);
}

#[test]
fn scan_marks_multi_stack_conflict_in_same_dir() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
    fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
    let result = xchendev_tools_lib::project_scanner::scan_project_directory(
        dir.path().to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(result.candidates.len(), 2);
    assert!(
        result
            .candidates
            .iter()
            .all(|c| c.status == xchendev_tools_lib::domain::CandidateStatus::Conflict)
    );
}
