use crate::domain::{CandidateStatus, ProjectScanResult, TechnologyCandidate, TechnologyStack};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SCAN_DEPTH: u8 = 3;
const SKIP_DIRECTORIES: &[&str] = &[
    ".git",
    ".idea",
    ".vscode",
    "node_modules",
    "target",
    "dist",
    "build",
];

const EVIDENCE_FILES: &[(&str, TechnologyStack)] = &[
    ("package.json", TechnologyStack::Node),
    ("pom.xml", TechnologyStack::Maven),
    ("build.gradle", TechnologyStack::Gradle),
    ("build.gradle.kts", TechnologyStack::Gradle),
    ("requirements.txt", TechnologyStack::Python),
    ("pyproject.toml", TechnologyStack::Python),
    ("composer.json", TechnologyStack::Php),
    ("Cargo.toml", TechnologyStack::Rust),
];

const NODE_SCRIPT_PRIORITY: &[&str] = &["dev", "start", "serve", "develop", "watch"];

pub fn scan_project_directory(root_path: &str) -> Result<ProjectScanResult, String> {
    let root = Path::new(root_path);
    if !root.exists() || !root.is_dir() {
        return Err("PROJECT_PATH_INVALID:项目路径无效".to_string());
    }
    let canonical = fs::canonicalize(root).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;

    let mut candidates = Vec::new();
    scan_tree(&canonical, &canonical, 1, &mut candidates)?;
    mark_conflicts(&mut candidates);

    Ok(ProjectScanResult {
        root_path: canonical.to_string_lossy().to_string(),
        scan_depth: MAX_SCAN_DEPTH,
        candidates,
    })
}

fn read_scan_entries(dir: &Path, level: u8) -> Result<Option<fs::ReadDir>, String> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if level == 1 => Err(format!("PROJECT_SCAN_FAILED:{}", error)),
        Err(_) => Ok(None),
    }
}

fn scan_tree(
    dir: &Path,
    root: &Path,
    level: u8,
    out: &mut Vec<TechnologyCandidate>,
) -> Result<(), String> {
    scan_dir(dir, root, out)?;
    if level >= MAX_SCAN_DEPTH {
        return Ok(());
    }

    let Some(entries) = read_scan_entries(dir, level)? else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if should_skip_directory(&path) {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }
        let Some(canonical_child) = canonical_child_within_root(&path, root) else {
            continue;
        };
        if !canonical_child.is_dir() {
            continue;
        }
        scan_tree(&canonical_child, root, level + 1, out)?;
    }
    Ok(())
}

fn canonical_child_within_root(path: &Path, root: &Path) -> Option<PathBuf> {
    let canonical = fs::canonicalize(path).ok()?;
    canonical.starts_with(root).then_some(canonical)
}

fn should_skip_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    SKIP_DIRECTORIES
        .iter()
        .any(|ignored| name.eq_ignore_ascii_case(ignored))
}

fn scan_dir(dir: &Path, root: &Path, out: &mut Vec<TechnologyCandidate>) -> Result<(), String> {
    for (file_name, stack) in EVIDENCE_FILES {
        let evidence = dir.join(file_name);
        if !evidence.is_file() {
            continue;
        }
        let candidate = build_candidate(dir, root, &evidence, *stack, file_name)?;
        let duplicate_stack = out.iter().any(|existing| {
            existing
                .directory
                .eq_ignore_ascii_case(&candidate.directory)
                && existing.stack == candidate.stack
        });
        if !duplicate_stack {
            out.push(candidate);
        }
    }
    Ok(())
}

fn build_candidate(
    dir: &Path,
    root: &Path,
    evidence: &PathBuf,
    stack: TechnologyStack,
    file_name: &str,
) -> Result<TechnologyCandidate, String> {
    let directory = dir.to_string_lossy().to_string();
    let evidence_file = evidence.to_string_lossy().to_string();
    let id = stable_candidate_id(root, dir, file_name, stack);

    match stack {
        TechnologyStack::Node => {
            let content =
                fs::read_to_string(evidence).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;
            let scripts = parse_npm_scripts(&content);
            let preferred = preferred_node_scripts(&scripts);
            let (status, suggested_command, conflict_group) = if preferred.len() == 1 {
                (
                    CandidateStatus::Ready,
                    Some(format!("npm run {}", preferred[0])),
                    None,
                )
            } else if preferred.len() > 1 {
                (
                    CandidateStatus::NeedsConfirmation,
                    Some(format!("npm run {}", preferred[0])),
                    Some(format!("node-scripts-{}", directory)),
                )
            } else if scripts.len() == 1 {
                (
                    CandidateStatus::Ready,
                    Some(format!("npm run {}", scripts[0])),
                    None,
                )
            } else if scripts.len() > 1 {
                (
                    CandidateStatus::Conflict,
                    None,
                    Some(format!("node-scripts-{}", directory)),
                )
            } else {
                (CandidateStatus::EvidenceOnly, None, None)
            };

            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status,
                suggested_command,
                scripts: Some(scripts),
                conflict_group,
            })
        }
        TechnologyStack::Maven => {
            let content =
                fs::read_to_string(evidence).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;
            let packaging_pom = xml_tag_value(&content, "packaging")
                .is_some_and(|value| value.eq_ignore_ascii_case("pom"));
            let is_spring_boot_module = is_spring_boot_maven(&content);
            let mvnw = dir.join("mvnw.cmd");
            let runner = if mvnw.is_file() { ".\\mvnw.cmd" } else { "mvn" };

            let (status, suggested_command) = if packaging_pom {
                (CandidateStatus::EvidenceOnly, None)
            } else if is_spring_boot_module {
                (
                    CandidateStatus::NeedsConfirmation,
                    Some(format!("{} spring-boot:run", runner)),
                )
            } else {
                (CandidateStatus::NeedsConfirmation, None)
            };

            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status,
                suggested_command,
                scripts: None,
                conflict_group: None,
            })
        }
        TechnologyStack::Gradle => {
            let content =
                fs::read_to_string(evidence).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;
            let is_spring_boot_module = content.contains("org.springframework.boot")
                || content.contains("spring-boot-gradle-plugin");
            let gradlew = dir.join("gradlew.bat");
            let suggested_command = if is_spring_boot_module {
                Some(if gradlew.is_file() {
                    ".\\gradlew.bat bootRun".to_string()
                } else {
                    "gradle bootRun".to_string()
                })
            } else {
                None
            };
            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status: CandidateStatus::NeedsConfirmation,
                suggested_command,
                scripts: None,
                conflict_group: None,
            })
        }
        TechnologyStack::Rust => {
            let is_binary = dir.join("src").join("main.rs").is_file();
            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status: if is_binary {
                    CandidateStatus::NeedsConfirmation
                } else {
                    CandidateStatus::EvidenceOnly
                },
                suggested_command: is_binary.then(|| "cargo run".to_string()),
                scripts: None,
                conflict_group: None,
            })
        }
        TechnologyStack::Python | TechnologyStack::Php => Ok(TechnologyCandidate {
            id,
            directory,
            evidence_file,
            stack,
            status: CandidateStatus::EvidenceOnly,
            suggested_command: None,
            scripts: None,
            conflict_group: None,
        }),
        TechnologyStack::Unknown => Ok(TechnologyCandidate {
            id,
            directory,
            evidence_file,
            stack: TechnologyStack::Unknown,
            status: CandidateStatus::EvidenceOnly,
            suggested_command: None,
            scripts: None,
            conflict_group: None,
        }),
    }
}

fn is_spring_boot_maven(content: &str) -> bool {
    content.contains("spring-boot-maven-plugin")
        || content.contains("spring-boot-starter-parent")
        || content.contains("spring-boot-starter-")
        || content.contains("spring-boot-dependencies")
}

fn xml_tag_value<'a>(content: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    let start = content.find(&start_tag)? + start_tag.len();
    let end = content[start..].find(&end_tag)? + start;
    Some(content[start..end].trim())
}

fn stable_candidate_id(root: &Path, dir: &Path, file_name: &str, stack: TechnologyStack) -> String {
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    let mut hasher = Sha256::new();
    hasher.update(relative.to_string_lossy().to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(file_name.as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{:?}", stack).as_bytes());
    format!("candidate-{}", &hex::encode(hasher.finalize())[..16])
}

fn parse_npm_scripts(content: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
    let mut scripts = Vec::new();
    if let Some(obj) = value.get("scripts").and_then(|s| s.as_object()) {
        scripts.extend(obj.keys().cloned());
    }
    scripts.sort();
    scripts
}

fn preferred_node_scripts(scripts: &[String]) -> Vec<String> {
    NODE_SCRIPT_PRIORITY
        .iter()
        .filter_map(|preferred| {
            scripts
                .iter()
                .find(|script| script.eq_ignore_ascii_case(preferred))
                .cloned()
        })
        .collect()
}

fn mark_conflicts(candidates: &mut [TechnologyCandidate]) {
    use std::collections::HashMap;

    let mut java_builds_by_dir: HashMap<String, (bool, bool)> = HashMap::new();
    for candidate in candidates.iter() {
        let entry = java_builds_by_dir
            .entry(candidate.directory.to_lowercase())
            .or_insert((false, false));
        match candidate.stack {
            TechnologyStack::Maven => entry.0 = true,
            TechnologyStack::Gradle => entry.1 = true,
            _ => {}
        }
    }

    for candidate in candidates.iter_mut() {
        let Some((has_maven, has_gradle)) = java_builds_by_dir
            .get(&candidate.directory.to_lowercase())
            .copied()
        else {
            continue;
        };
        if has_maven
            && has_gradle
            && matches!(
                candidate.stack,
                TechnologyStack::Maven | TechnologyStack::Gradle
            )
        {
            candidate.status = CandidateStatus::Conflict;
            candidate.conflict_group = Some(format!("java-build-system-{}", candidate.directory));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn node_script_priority_prefers_dev_before_build() {
        let scripts = vec!["build".to_string(), "dev".to_string(), "test".to_string()];
        assert_eq!(preferred_node_scripts(&scripts), vec!["dev".to_string()]);
    }

    #[test]
    fn node_script_priority_preserves_known_runtime_order() {
        let scripts = vec!["serve".to_string(), "start".to_string(), "dev".to_string()];
        assert_eq!(
            preferred_node_scripts(&scripts),
            vec!["dev".to_string(), "start".to_string(), "serve".to_string()]
        );
    }

    #[test]
    fn root_scan_read_error_is_fatal_but_nested_read_error_is_skipped() {
        let root = tempdir().expect("root");
        let missing = root.path().join("missing");

        assert!(read_scan_entries(&missing, 1).is_err());
        assert!(read_scan_entries(&missing, 2)
            .expect("nested policy")
            .is_none());
    }

    #[test]
    fn canonical_child_rejects_path_outside_project_root() {
        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        let canonical_root = fs::canonicalize(root.path()).expect("canonical root");
        assert!(canonical_child_within_root(outside.path(), &canonical_root).is_none());
    }

    #[test]
    fn scan_depth_three_finds_nested_module_and_skips_build_directories() {
        let root = tempdir().expect("tempdir");
        let backend = root.path().join("backend");
        let starter = backend.join("starter");
        let ignored = backend.join("target").join("generated");
        fs::create_dir_all(&starter).expect("starter dir");
        fs::create_dir_all(&ignored).expect("ignored dir");
        fs::write(
            starter.join("pom.xml"),
            "<project><packaging>jar</packaging><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>",
        )
        .expect("starter pom");
        fs::write(
            ignored.join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .expect("ignored package");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.scan_depth, MAX_SCAN_DEPTH);
        assert_eq!(result.candidates.len(), 1);
        assert!(result.candidates[0].directory.ends_with("starter"));
        assert_eq!(
            result.candidates[0].status,
            CandidateStatus::NeedsConfirmation
        );
        assert_eq!(
            result.candidates[0].suggested_command.as_deref(),
            Some("mvn spring-boot:run")
        );
    }

    #[test]
    fn maven_aggregator_pom_is_evidence_only() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join("pom.xml"),
            "<project><packaging>pom</packaging><modules><module>starter</module></modules></project>",
        )
        .expect("pom");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.status, CandidateStatus::EvidenceOnly);
        assert!(candidate.suggested_command.is_none());
    }

    #[test]
    fn spring_boot_maven_module_requires_confirmation() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join("pom.xml"),
            "<project><packaging>jar</packaging><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>",
        )
        .expect("pom");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.status, CandidateStatus::NeedsConfirmation);
        assert_eq!(
            candidate.suggested_command.as_deref(),
            Some("mvn spring-boot:run")
        );
    }

    #[test]
    fn spring_boot_parent_or_starter_is_recognized_without_plugin_block() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join("pom.xml"),
            "<project><parent><artifactId>spring-boot-starter-parent</artifactId></parent><dependencies><dependency><artifactId>spring-boot-starter-web</artifactId></dependency></dependencies></project>",
        )
        .expect("pom");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.stack, TechnologyStack::Maven);
        assert_eq!(
            candidate.suggested_command.as_deref(),
            Some("mvn spring-boot:run")
        );
    }

    #[test]
    fn gradle_kotlin_dsl_spring_boot_gets_bootrun_suggestion() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join("build.gradle.kts"),
            "plugins { id(\"org.springframework.boot\") version \"3.5.0\" }",
        )
        .expect("gradle kts");
        fs::write(root.path().join("gradlew.bat"), "@echo off").expect("wrapper");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.stack, TechnologyStack::Gradle);
        assert_eq!(
            candidate.suggested_command.as_deref(),
            Some(".\\gradlew.bat bootRun")
        );
    }

    #[test]
    fn plain_gradle_project_does_not_invent_bootrun() {
        let root = tempdir().expect("tempdir");
        fs::write(root.path().join("build.gradle"), "plugins { id 'java' }").expect("gradle");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.stack, TechnologyStack::Gradle);
        assert!(candidate.suggested_command.is_none());
    }

    #[test]
    fn rust_binary_project_suggests_cargo_run() {
        let root = tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src");
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .expect("cargo");
        fs::write(root.path().join("src").join("main.rs"), "fn main() {}\n").expect("main");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.stack, TechnologyStack::Rust);
        assert_eq!(candidate.status, CandidateStatus::NeedsConfirmation);
        assert_eq!(candidate.suggested_command.as_deref(), Some("cargo run"));
    }

    #[test]
    fn rust_library_project_is_evidence_only() {
        let root = tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src");
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='demo-lib'\nversion='0.1.0'\n",
        )
        .expect("cargo");
        fs::write(root.path().join("src").join("lib.rs"), "pub fn demo() {}\n").expect("lib");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        let candidate = result.candidates.first().expect("candidate");
        assert_eq!(candidate.stack, TechnologyStack::Rust);
        assert_eq!(candidate.status, CandidateStatus::EvidenceOnly);
        assert!(candidate.suggested_command.is_none());
    }

    #[test]
    fn multiple_evidence_files_for_same_stack_are_deduplicated() {
        let root = tempdir().expect("tempdir");
        fs::write(root.path().join("requirements.txt"), "fastapi\n").expect("requirements");
        fs::write(
            root.path().join("pyproject.toml"),
            "[project]\nname='demo'\n",
        )
        .expect("pyproject");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].stack, TechnologyStack::Python);
        assert_eq!(result.candidates[0].status, CandidateStatus::EvidenceOnly);
        assert!(result.candidates[0].conflict_group.is_none());
    }

    #[test]
    fn node_and_rust_same_directory_are_not_forced_to_conflict() {
        let root = tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src");
        fs::write(
            root.path().join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .expect("package");
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='desktop'\nversion='0.1.0'\n",
        )
        .expect("cargo");
        fs::write(root.path().join("src").join("main.rs"), "fn main() {}\n").expect("main");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.candidates.len(), 2);
        let node = result
            .candidates
            .iter()
            .find(|candidate| candidate.stack == TechnologyStack::Node)
            .expect("node candidate");
        let rust = result
            .candidates
            .iter()
            .find(|candidate| candidate.stack == TechnologyStack::Rust)
            .expect("rust candidate");
        assert_eq!(node.status, CandidateStatus::Ready);
        assert_eq!(rust.status, CandidateStatus::NeedsConfirmation);
        assert!(node.conflict_group.is_none());
        assert!(rust.conflict_group.is_none());
    }

    #[test]
    fn maven_and_gradle_same_directory_are_conflict() {
        let root = tempdir().expect("tempdir");
        fs::write(
            root.path().join("pom.xml"),
            "<project><packaging>jar</packaging></project>",
        )
        .expect("pom");
        fs::write(root.path().join("build.gradle"), "plugins { id 'java' }").expect("gradle");

        let result =
            scan_project_directory(root.path().to_str().expect("root path")).expect("scan");
        assert_eq!(result.candidates.len(), 2);
        for candidate in &result.candidates {
            assert_eq!(candidate.status, CandidateStatus::Conflict);
            assert!(candidate
                .conflict_group
                .as_deref()
                .is_some_and(|group| group.starts_with("java-build-system-")));
        }
    }

    #[test]
    fn fullstack_multimodule_layout_is_stable_across_rescans() {
        let root = tempdir().expect("tempdir");
        let web = root.path().join("xingyu-web");
        let backend = root.path().join("xingyu-backend");
        let starter = backend.join("xingyu-starter");
        fs::create_dir_all(&web).expect("web");
        fs::create_dir_all(&starter).expect("starter");

        fs::write(
            web.join("package.json"),
            r#"{"scripts":{"build":"vite build","dev":"vite","start":"vite preview"}}"#,
        )
        .expect("web package");
        fs::write(
            backend.join("pom.xml"),
            "<project><packaging>pom</packaging><modules><module>xingyu-starter</module></modules></project>",
        )
        .expect("parent pom");
        fs::write(
            starter.join("pom.xml"),
            "<project><packaging>jar</packaging><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>",
        )
        .expect("starter pom");

        let root_str = root.path().to_str().expect("root path");
        let first = scan_project_directory(root_str).expect("first scan");
        let second = scan_project_directory(root_str).expect("second scan");
        assert_eq!(first.candidates.len(), 3);

        let web_candidate = first
            .candidates
            .iter()
            .find(|candidate| candidate.directory.ends_with("xingyu-web"))
            .expect("web candidate");
        assert_eq!(web_candidate.stack, TechnologyStack::Node);
        assert_eq!(web_candidate.status, CandidateStatus::NeedsConfirmation);
        assert_eq!(
            web_candidate.suggested_command.as_deref(),
            Some("npm run dev")
        );

        let parent_candidate = first
            .candidates
            .iter()
            .find(|candidate| candidate.directory.ends_with("xingyu-backend"))
            .expect("parent candidate");
        assert_eq!(parent_candidate.stack, TechnologyStack::Maven);
        assert_eq!(parent_candidate.status, CandidateStatus::EvidenceOnly);
        assert!(parent_candidate.suggested_command.is_none());

        let starter_candidate = first
            .candidates
            .iter()
            .find(|candidate| candidate.directory.ends_with("xingyu-starter"))
            .expect("starter candidate");
        assert_eq!(starter_candidate.stack, TechnologyStack::Maven);
        assert_eq!(starter_candidate.status, CandidateStatus::NeedsConfirmation);
        assert_eq!(
            starter_candidate.suggested_command.as_deref(),
            Some("mvn spring-boot:run")
        );

        let mut first_ids: Vec<_> = first
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        let mut second_ids: Vec<_> = second
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        first_ids.sort();
        second_ids.sort();
        assert_eq!(
            first_ids, second_ids,
            "candidate ids must be stable across rescans"
        );
    }
}
