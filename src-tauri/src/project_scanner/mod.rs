use crate::domain::{
    CandidateStatus, ProjectScanResult, TechnologyCandidate, TechnologyStack,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const EVIDENCE_FILES: &[(&str, TechnologyStack)] = &[
    ("package.json", TechnologyStack::Node),
    ("pom.xml", TechnologyStack::Maven),
    ("build.gradle", TechnologyStack::Gradle),
    ("requirements.txt", TechnologyStack::Python),
    ("pyproject.toml", TechnologyStack::Python),
    ("composer.json", TechnologyStack::Php),
];

const NODE_SCRIPT_PRIORITY: &[&str] = &["dev", "start", "serve", "develop", "watch"];

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
    _level: u8,
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
            let content = fs::read_to_string(evidence).map_err(|e| format!("PROJECT_SCAN_FAILED:{}", e))?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
