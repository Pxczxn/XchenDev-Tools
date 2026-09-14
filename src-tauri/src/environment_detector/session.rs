use crate::domain::{DetectionSource, EnvironmentCandidate, ValidationStatus};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use super::{
    env_path_candidates, extract_version, insert_path, registry_candidates,
    should_include_in_list, source_priority, RUNTIME_KINDS,
};

pub struct DetectionSession {
    where_results: HashMap<String, Vec<String>>,
    validation_cache: Arc<Mutex<HashMap<String, EnvironmentCandidate>>>,
}

impl DetectionSession {
    pub fn new(where_results: HashMap<String, Vec<String>>) -> Self {
        Self {
            where_results,
            validation_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn batch_where_commands(names: &[&str]) -> HashMap<String, Vec<String>> {
        let mut unique = HashSet::new();
        for name in names {
            unique.insert(*name);
        }
        let mut out = HashMap::new();
        for name in unique {
            out.insert(name.to_string(), where_command(name).unwrap_or_default());
        }
        out
    }

    pub fn collect_kind(
        &self,
        kind: &str,
        names: &[&str],
        manual_override: Option<&str>,
        path_hint: Option<&str>,
    ) -> Vec<EnvironmentCandidate> {
        let mut path_sources: HashMap<String, DetectionSource> = HashMap::new();

        if let Some(manual) = manual_override {
            insert_path(
                &mut path_sources,
                manual.to_string(),
                DetectionSource::ManualOverride,
            );
        }

        let mut native_hit = false;
        for name in names {
            if let Some(paths) = self.where_results.get(*name) {
                if !paths.is_empty() {
                    native_hit = true;
                }
                for path in paths {
                    insert_path(
                        &mut path_sources,
                        path.clone(),
                        DetectionSource::NativeCommand,
                    );
                }
            }
        }

        if let Some(hint) = path_hint {
            insert_path(
                &mut path_sources,
                hint.to_string(),
                DetectionSource::EnvironmentVariable,
            );
        }

        if !native_hit {
            for path in env_path_candidates(names) {
                insert_path(
                    &mut path_sources,
                    path,
                    DetectionSource::EnvironmentVariable,
                );
            }
        }

        for path in registry_candidates(kind) {
            insert_path(&mut path_sources, path, DetectionSource::Registry);
        }

        let mut entries: Vec<(String, DetectionSource)> = path_sources.into_iter().collect();
        entries.sort_by(|a, b| {
            source_priority(&a.1)
                .cmp(&source_priority(&b.1))
                .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });

        let mut kind_results = Vec::new();
        let mut seen_resolved = HashSet::new();
        let mut kind_has_candidate = false;

        for (path, source) in entries {
            let user_configured = source == DetectionSource::ManualOverride;
            if let Some(c) = self.validate_cached(kind, &path, source, user_configured) {
                if let Some(resolved) = c.resolved_path.as_deref() {
                    if !seen_resolved.insert(resolved.to_lowercase()) {
                        continue;
                    }
                }
                if should_include_in_list(&c) {
                    kind_has_candidate = true;
                    kind_results.push(c);
                }
            }
        }

        if !kind_has_candidate {
            kind_results.push(EnvironmentCandidate {
                runtime_kind: kind.to_string(),
                source: DetectionSource::EnvironmentVariable,
                executable_path: None,
                resolved_path: None,
                version: None,
                validation_status: ValidationStatus::Unknown,
                validation_reason: Some("未找到可用候选".to_string()),
                is_user_configured: false,
            });
        }

        kind_results
    }

    pub fn validate_one(
        &self,
        kind: &str,
        path: &str,
        source: DetectionSource,
        user_configured: bool,
    ) -> Option<EnvironmentCandidate> {
        self.validate_cached(kind, path, source, user_configured)
    }

    fn validate_cached(
        &self,
        kind: &str,
        path: &str,
        source: DetectionSource,
        user_configured: bool,
    ) -> Option<EnvironmentCandidate> {
        let cache_key = format!("{}|{}", kind, normalize_path_key(path));
        if let Ok(cache) = self.validation_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Some(apply_source(cached, source, user_configured));
            }
        }

        let result = validate_executable(kind, path, source.clone(), user_configured);
        if let Ok(mut cache) = self.validation_cache.lock() {
            if let Some(ref candidate) = result {
                cache.insert(cache_key, candidate.clone());
            }
        }
        result
    }
}

fn apply_source(
    cached: &EnvironmentCandidate,
    source: DetectionSource,
    user_configured: bool,
) -> EnvironmentCandidate {
    EnvironmentCandidate {
        source,
        is_user_configured: user_configured,
        ..cached.clone()
    }
}

fn normalize_path_key(path: &str) -> String {
    let p = Path::new(path);
    std::fs::canonicalize(p)
        .map(|resolved| resolved.to_string_lossy().to_string().to_lowercase())
        .unwrap_or_else(|_| path.to_lowercase())
}

fn where_command(name: &str) -> Option<Vec<String>> {
    let output = Command::new("where").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

fn validate_executable(
    kind: &str,
    path: &str,
    source: DetectionSource,
    user_configured: bool,
) -> Option<EnvironmentCandidate> {
    let p = Path::new(path);
    if !p.is_file() {
        return Some(EnvironmentCandidate {
            runtime_kind: kind.to_string(),
            source,
            executable_path: Some(path.to_string()),
            resolved_path: None,
            version: None,
            validation_status: ValidationStatus::Invalid,
            validation_reason: Some("路径不是可执行文件".to_string()),
            is_user_configured: user_configured,
        });
    }

    let version_flag = RUNTIME_KINDS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, _, f)| *f)
        .unwrap_or("--version");

    let output = Command::new(path).arg(version_flag).output();
    match output {
        Ok(out) if out.status.success() || !out.stderr.is_empty() || !out.stdout.is_empty() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let err = String::from_utf8_lossy(&out.stderr);
            let version = extract_version(&text, &err);
            let resolved = std::fs::canonicalize(p)
                .ok()
                .map(|pb| pb.to_string_lossy().to_string());
            if resolved.is_none() {
                return Some(EnvironmentCandidate {
                    runtime_kind: kind.to_string(),
                    source,
                    executable_path: Some(path.to_string()),
                    resolved_path: None,
                    version,
                    validation_status: ValidationStatus::Invalid,
                    validation_reason: Some("无法解析实际路径".to_string()),
                    is_user_configured: user_configured,
                });
            }
            Some(EnvironmentCandidate {
                runtime_kind: kind.to_string(),
                source,
                executable_path: Some(path.to_string()),
                resolved_path: resolved,
                version,
                validation_status: ValidationStatus::Valid,
                validation_reason: None,
                is_user_configured: user_configured,
            })
        }
        Ok(_) => Some(EnvironmentCandidate {
            runtime_kind: kind.to_string(),
            source,
            executable_path: Some(path.to_string()),
            resolved_path: None,
            version: None,
            validation_status: ValidationStatus::Invalid,
            validation_reason: Some("版本命令执行失败".to_string()),
            is_user_configured: user_configured,
        }),
        Err(e) => Some(EnvironmentCandidate {
            runtime_kind: kind.to_string(),
            source,
            executable_path: Some(path.to_string()),
            resolved_path: None,
            version: None,
            validation_status: ValidationStatus::Invalid,
            validation_reason: Some(e.to_string()),
            is_user_configured: user_configured,
        }),
    }
}
