mod session;

use crate::config_store::ConfigStore;
use crate::domain::{DetectionSource, EnvironmentCandidate, ValidationStatus};
use session::DetectionSession;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub(super) const RUNTIME_KINDS: &[(&str, &[&str], &str)] = &[
    ("java", &["java"], "-version"),
    ("python", &["python", "python3"], "--version"),
    ("node", &["node"], "--version"),
    ("php", &["php"], "--version"),
    ("rust", &["rustc"], "--version"),
];

pub fn all_runtime_kind_ids() -> Vec<&'static str> {
    RUNTIME_KINDS.iter().map(|(k, _, _)| *k).collect()
}

pub fn resolve_runtime_kinds(requested: &[String], disabled: &[String]) -> Vec<&'static str> {
    let disabled_set: HashSet<String> = disabled
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let base: Vec<&'static str> = if requested.is_empty() {
        all_runtime_kind_ids()
    } else {
        requested
            .iter()
            .filter_map(|s| {
                let key = s.trim().to_lowercase();
                RUNTIME_KINDS
                    .iter()
                    .find(|(id, _, _)| *id == key)
                    .map(|(id, _, _)| *id)
            })
            .collect()
    };
    base.into_iter()
        .filter(|k| !disabled_set.contains(&k.to_lowercase()))
        .collect()
}

pub fn list_candidates(
    store: &ConfigStore,
    runtime_kinds: &[String],
) -> Result<Vec<EnvironmentCandidate>, String> {
    let settings = store.get_settings();
    let kinds = resolve_runtime_kinds(runtime_kinds, &settings.disabled_runtime_kinds);

    let manual_overrides = store.all_manual_overrides();
    let hints = settings.detection_path_hints;

    let mut lookup_names: Vec<&str> = Vec::new();
    for kind in &kinds {
        if let Some((_, names, _)) = RUNTIME_KINDS.iter().find(|(k, _, _)| k == kind) {
            lookup_names.extend(*names);
        }
    }

    let where_results = DetectionSession::batch_where_commands(&lookup_names);
    let session = Arc::new(DetectionSession::new(where_results));

    let mut out = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for kind in kinds {
            let spec = RUNTIME_KINDS.iter().find(|(k, _, _)| *k == kind);
            if spec.is_none() {
                continue;
            }
            let (_, names, _) = spec.unwrap();
            let manual = manual_overrides
                .get(kind)
                .map(|path| path.as_str())
                .unwrap_or("");
            let manual = if manual.is_empty() {
                None
            } else {
                Some(manual)
            };
            let hint = hints.get(kind).map(|path| path.as_str());
            let session = Arc::clone(&session);
            handles.push(scope.spawn(move || session.collect_kind(kind, names, manual, hint)));
        }
        for handle in handles {
            if let Ok(mut chunk) = handle.join() {
                out.append(&mut chunk);
            }
        }
    });

    sort_candidates(&mut out);
    Ok(out)
}

pub fn validate_and_save_override(
    store: &ConfigStore,
    runtime_kind: &str,
    executable_path: &str,
) -> Result<EnvironmentCandidate, String> {
    let path = Path::new(executable_path);
    if !path.exists() {
        return Err("PATH_NOT_FOUND:路径不存在".to_string());
    }
    let session = DetectionSession::new(HashMap::new());
    let candidate = session
        .validate_one(
            runtime_kind,
            executable_path,
            DetectionSource::ManualOverride,
            true,
        )
        .ok_or_else(|| "EXECUTABLE_INVALID:无法验证可执行文件".to_string())?;
    if candidate.validation_status != ValidationStatus::Valid {
        return Err(format!(
            "EXECUTABLE_INVALID:{}",
            candidate.validation_reason.unwrap_or_default()
        ));
    }
    store.save_manual_override(runtime_kind, executable_path)?;
    Ok(candidate)
}

pub fn should_include_in_list(candidate: &EnvironmentCandidate) -> bool {
    candidate.validation_status != ValidationStatus::Invalid || candidate.is_user_configured
}

pub fn source_priority(source: &DetectionSource) -> u8 {
    match source {
        DetectionSource::ManualOverride => 0,
        DetectionSource::NativeCommand => 1,
        DetectionSource::EnvironmentVariable => 2,
        DetectionSource::Registry => 3,
    }
}

fn validation_priority(status: &ValidationStatus) -> u8 {
    match status {
        ValidationStatus::Valid => 0,
        ValidationStatus::Invalid => 1,
        ValidationStatus::Unknown => 2,
    }
}

pub fn sort_candidates(candidates: &mut [EnvironmentCandidate]) {
    candidates.sort_by(|a, b| {
        a.runtime_kind
            .cmp(&b.runtime_kind)
            .then(source_priority(&a.source).cmp(&source_priority(&b.source)))
            .then(
                validation_priority(&a.validation_status)
                    .cmp(&validation_priority(&b.validation_status)),
            )
            .then(path_key(a).cmp(&path_key(b)))
    });
}

fn path_key(candidate: &EnvironmentCandidate) -> String {
    candidate
        .resolved_path
        .as_deref()
        .or(candidate.executable_path.as_deref())
        .unwrap_or("")
        .to_lowercase()
}

pub(super) fn insert_path(
    map: &mut HashMap<String, DetectionSource>,
    path: String,
    source: DetectionSource,
) {
    map.entry(path)
        .and_modify(|existing| {
            if source_priority(&source) < source_priority(existing) {
                *existing = source.clone();
            }
        })
        .or_insert(source);
}

pub(super) fn extract_version(stdout: &str, stderr: &str) -> Option<String> {
    let combined = format!("{}\n{}", stdout, stderr);
    let mut fallback = None;
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Picked up JAVA_TOOL_OPTIONS") {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.contains("version")
            || lower.starts_with("python ")
            || lower.starts_with("node ")
            || lower.starts_with("php ")
            || lower.starts_with("rustc ")
            || lower.starts_with("cargo ")
            || trimmed.chars().any(|c| c.is_ascii_digit())
        {
            return Some(trimmed.chars().take(80).collect());
        }
        if fallback.is_none() {
            fallback = Some(trimmed.chars().take(80).collect());
        }
    }
    fallback
}

pub(super) fn env_path_candidates(names: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(';') {
        let dir = dir.trim();
        if dir.is_empty() {
            continue;
        }
        for name in names {
            let candidate = Path::new(dir).join(format!("{}.exe", name));
            if candidate.is_file() {
                result.push(candidate.to_string_lossy().to_string());
            }
        }
    }
    result
}

pub(super) fn registry_candidates(kind: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if kind == "java" {
        use winreg::enums::*;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(java) = hklm.open_subkey("SOFTWARE\\JavaSoft\\JDK") {
            for name in java.enum_keys().flatten() {
                if let Ok(ver) = java.open_subkey(&name) {
                    if let Ok(home) = ver.get_value::<String, _>("JavaHome") {
                        let exe = Path::new(&home).join("bin").join("java.exe");
                        if exe.is_file() {
                            paths.push(exe.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    if kind == "rust" {
        push_rust_toolchain_candidates(&mut paths);
    }
    paths
}

pub(super) fn push_rust_toolchain_candidates(paths: &mut Vec<String>) {
    let cargo_bins = [
        std::env::var("CARGO_HOME")
            .ok()
            .map(|home| Path::new(&home).join("bin")),
        std::env::var("USERPROFILE")
            .ok()
            .map(|home| Path::new(&home).join(".cargo").join("bin")),
    ];
    for bin_dir in cargo_bins.into_iter().flatten() {
        let rustc = bin_dir.join("rustc.exe");
        if rustc.is_file() {
            paths.push(rustc.to_string_lossy().to_string());
        }
    }

    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(rustup) = hkcu.open_subkey("Software\\Rust\\Rustup") {
        if let Ok(home) = rustup.get_value::<String, _>("Home") {
            let toolchain_dir = Path::new(&home).join("toolchains");
            if toolchain_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&toolchain_dir) {
                    for entry in entries.flatten() {
                        let rustc = entry.path().join("bin").join("rustc.exe");
                        if rustc.is_file() {
                            paths.push(rustc.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DetectionSource, ValidationStatus};

    #[test]
    fn invalid_auto_candidate_is_excluded_from_list() {
        let invalid = EnvironmentCandidate {
            runtime_kind: "python".to_string(),
            source: DetectionSource::NativeCommand,
            executable_path: Some("C:\\WindowsApps\\python.exe".to_string()),
            resolved_path: None,
            version: None,
            validation_status: ValidationStatus::Invalid,
            validation_reason: Some("无法解析实际路径".to_string()),
            is_user_configured: false,
        };
        assert!(!should_include_in_list(&invalid));

        let manual_invalid = EnvironmentCandidate {
            is_user_configured: true,
            source: DetectionSource::ManualOverride,
            ..invalid.clone()
        };
        assert!(should_include_in_list(&manual_invalid));
    }

    #[test]
    fn source_priority_orders_native_before_env_before_registry() {
        assert!(
            source_priority(&DetectionSource::NativeCommand)
                < source_priority(&DetectionSource::EnvironmentVariable)
        );
        assert!(
            source_priority(&DetectionSource::EnvironmentVariable)
                < source_priority(&DetectionSource::Registry)
        );
        assert!(
            source_priority(&DetectionSource::ManualOverride)
                < source_priority(&DetectionSource::NativeCommand)
        );
    }

    #[test]
    fn insert_path_keeps_manual_over_lower_priority_source() {
        let mut map = HashMap::new();
        insert_path(
            &mut map,
            "C:\\Tools\\node.exe".to_string(),
            DetectionSource::NativeCommand,
        );
        insert_path(
            &mut map,
            "C:\\Tools\\node.exe".to_string(),
            DetectionSource::ManualOverride,
        );
        assert_eq!(
            map.get("C:\\Tools\\node.exe"),
            Some(&DetectionSource::ManualOverride)
        );
    }

    #[test]
    fn sort_candidates_puts_manual_first() {
        let mut list = vec![
            EnvironmentCandidate {
                runtime_kind: "node".to_string(),
                source: DetectionSource::NativeCommand,
                executable_path: Some("C:\\cmd\\node.exe".to_string()),
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Valid,
                validation_reason: None,
                is_user_configured: false,
            },
            EnvironmentCandidate {
                runtime_kind: "node".to_string(),
                source: DetectionSource::ManualOverride,
                executable_path: Some("D:\\manual\\node.exe".to_string()),
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Valid,
                validation_reason: None,
                is_user_configured: true,
            },
        ];
        sort_candidates(&mut list);
        assert_eq!(list[0].source, DetectionSource::ManualOverride);
    }

    #[test]
    fn insert_path_keeps_highest_priority_source() {
        let mut map = HashMap::new();
        insert_path(
            &mut map,
            "C:\\Tools\\node.exe".to_string(),
            DetectionSource::Registry,
        );
        insert_path(
            &mut map,
            "C:\\Tools\\node.exe".to_string(),
            DetectionSource::NativeCommand,
        );
        assert_eq!(
            map.get("C:\\Tools\\node.exe"),
            Some(&DetectionSource::NativeCommand)
        );
    }

    #[test]
    fn resolve_runtime_kinds_excludes_disabled() {
        let all = resolve_runtime_kinds(&[], &["php".to_string()]);
        assert!(!all.contains(&"php"));
        assert!(all.contains(&"java"));
        let none = resolve_runtime_kinds(&[], &["php".to_string(), "java".to_string()]);
        assert!(!none.contains(&"php"));
        assert!(!none.contains(&"java"));
    }

    #[test]
    fn sort_candidates_orders_by_source_then_validity() {
        let mut list = vec![
            EnvironmentCandidate {
                runtime_kind: "node".to_string(),
                source: DetectionSource::Registry,
                executable_path: Some("C:\\reg\\node.exe".to_string()),
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Valid,
                validation_reason: None,
                is_user_configured: false,
            },
            EnvironmentCandidate {
                runtime_kind: "node".to_string(),
                source: DetectionSource::NativeCommand,
                executable_path: Some("C:\\cmd\\node.exe".to_string()),
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Invalid,
                validation_reason: Some("bad".to_string()),
                is_user_configured: false,
            },
            EnvironmentCandidate {
                runtime_kind: "node".to_string(),
                source: DetectionSource::EnvironmentVariable,
                executable_path: Some("C:\\env\\node.exe".to_string()),
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Valid,
                validation_reason: None,
                is_user_configured: false,
            },
        ];
        sort_candidates(&mut list);
        assert_eq!(list[0].source, DetectionSource::NativeCommand);
        assert_eq!(list[1].source, DetectionSource::EnvironmentVariable);
        assert_eq!(list[2].source, DetectionSource::Registry);
    }
}
