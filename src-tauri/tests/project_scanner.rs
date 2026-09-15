use std::fs;
use tempfile::tempdir;

#[test]
fn scan_finds_package_json_at_root() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    let result =
        xchendev_tools_lib::project_scanner::scan_project_directory(dir.path().to_str().unwrap())
            .unwrap();
    assert!(!result.candidates.is_empty());
    assert_eq!(
        result.candidates[0].stack,
        xchendev_tools_lib::domain::TechnologyStack::Node
    );
}

#[test]
fn scan_allows_node_and_rust_in_same_dir() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='desktop'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    let result =
        xchendev_tools_lib::project_scanner::scan_project_directory(dir.path().to_str().unwrap())
            .unwrap();
    assert_eq!(result.candidates.len(), 2);
    assert!(result.candidates.iter().all(|candidate| {
        candidate.status != xchendev_tools_lib::domain::CandidateStatus::Conflict
            && candidate.conflict_group.is_none()
    }));
}

#[test]
fn scan_marks_maven_and_gradle_conflict_in_same_dir() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("pom.xml"),
        "<project><packaging>jar</packaging></project>",
    )
    .unwrap();
    fs::write(dir.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

    let result =
        xchendev_tools_lib::project_scanner::scan_project_directory(dir.path().to_str().unwrap())
            .unwrap();
    assert_eq!(result.candidates.len(), 2);
    assert!(result.candidates.iter().all(|candidate| {
        candidate.status == xchendev_tools_lib::domain::CandidateStatus::Conflict
            && candidate
                .conflict_group
                .as_deref()
                .is_some_and(|group| group.starts_with("java-build-system-"))
    }));
}
