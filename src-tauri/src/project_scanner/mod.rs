use crate::domain::{
    CandidateStatus, ProjectScanResult, TechnologyCandidate, TechnologyStack,
};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const EVIDENCE_FILES: &[(&str, TechnologyStack)] = &[
    ("package.json", TechnologyStack::Node),
    ("pom.xml", TechnologyStack::Maven),
    ("build.gradle", TechnologyStack::Gradle),
    ("requirements.txt", TechnologyStack::Python),
    ("pyproject.toml", TechnologyStack::Python),
    ("composer.json", TechnologyStack::Php),
];

pub fn scan_project_directory(root_path: &str) -> Result<ProjectScanResult, String> {
    let root = Path::new(root_path);
    if !root.exists() || !root.is_dir() {
        return Err("PROJECT_PATH_INVALID:项目路径无效".to_string());
    }
    let canonical = fs::canonicalize(root).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;

    let mut candidates = Vec::new();
    scan_dir(&canonical, &canonical, 1, &mut candidates)?;
    if let Ok(entries) = fs::read_dir(&canonical) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, &canonical, 2, &mut candidates)?;
            }
        }
    }

    mark_conflicts(&mut candidates);

    Ok(ProjectScanResult {
        root_path: canonical.to_string_lossy().to_string(),
        scan_depth: 2,
        candidates,
    })
}

fn scan_dir(
    dir: &Path,
    root: &Path,
    level: u8,
    out: &mut Vec<TechnologyCandidate>,
) -> Result<(), String> {
    for (file_name, stack) in EVIDENCE_FILES {
        let evidence = dir.join(file_name);
        if !evidence.is_file() {
            continue;
        }
        let candidate = build_candidate(dir, root, &evidence, *stack, file_name)?;
        out.push(candidate);
    }
    Ok(())
}

fn build_candidate(
    dir: &Path,
    _root: &Path,
    evidence: &PathBuf,
    stack: TechnologyStack,
    file_name: &str,
) -> Result<TechnologyCandidate, String> {
    let id = Uuid::new_v4().to_string();
    let directory = dir.to_string_lossy().to_string();
    let evidence_file = evidence.to_string_lossy().to_string();

    match stack {
        TechnologyStack::Node => {
            let content = fs::read_to_string(evidence).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;
            let scripts = parse_npm_scripts(&content);
            let suggested = scripts.first().cloned();
            let conflict = scripts.len() > 1;
            let dir_label = directory.clone();
            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status: if conflict {
                    CandidateStatus::Conflict
                } else {
                    CandidateStatus::Ready
                },
                suggested_command: suggested.map(|s| format!("npm run {}", s)),
                scripts: Some(scripts),
                conflict_group: if conflict {
                    Some(format!("node-scripts-{}", dir_label))
                } else {
                    None
                },
            })
        }
        TechnologyStack::Maven => {
            let mvnw = dir.join("mvnw.cmd");
            let cmd = if mvnw.is_file() {
                ".\\mvnw.cmd spring-boot:run".to_string()
            } else {
                "mvn spring-boot:run".to_string()
            };
            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status: CandidateStatus::NeedsConfirmation,
                suggested_command: Some(cmd),
                scripts: None,
                conflict_group: None,
            })
        }
        TechnologyStack::Gradle => {
            let gradlew = dir.join("gradlew.bat");
            let cmd = if gradlew.is_file() {
                ".\\gradlew.bat bootRun".to_string()
            } else {
                "gradle bootRun".to_string()
            };
            Ok(TechnologyCandidate {
                id,
                directory,
                evidence_file,
                stack,
                status: CandidateStatus::NeedsConfirmation,
                suggested_command: Some(cmd),
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

fn parse_npm_scripts(content: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
    let mut scripts = Vec::new();
    if let Some(obj) = value.get("scripts").and_then(|s| s.as_object()) {
        for key in obj.keys() {
            scripts.push(key.clone());
        }
    }
    scripts.sort();
    scripts
}

fn mark_conflicts(candidates: &mut [TechnologyCandidate]) {
    use std::collections::HashMap;
    let mut by_dir: HashMap<String, usize> = HashMap::new();
    for c in candidates.iter() {
        *by_dir.entry(c.directory.clone()).or_insert(0) += 1;
    }
    for c in candidates.iter_mut() {
        if by_dir.get(&c.directory).copied().unwrap_or(0) > 1 {
            c.status = CandidateStatus::Conflict;
            c.conflict_group = Some(format!("multi-stack-{}", c.directory));
        }
    }
}
