mod atomic;

use atomic::{atomic_write_with_backup, backup_path};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::{AppSettings, AuditEvent, LaunchProfile, ProjectInfo, RecentError};

const CURRENT_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub manual_overrides: HashMap<String, String>,
    #[serde(default)]
    pub projects: Vec<ProjectInfo>,
    #[serde(default)]
    pub launch_profiles: Vec<LaunchProfile>,
    #[serde(default)]
    pub audit_events: Vec<AuditEvent>,
    #[serde(default)]
    pub recent_errors: Vec<RecentError>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
            settings: AppSettings::default(),
            manual_overrides: HashMap::new(),
            projects: vec![],
            launch_profiles: vec![],
            audit_events: vec![],
            recent_errors: vec![],
        }
    }
}

pub struct ConfigStore {
    path: PathBuf,
    pub config: Mutex<AppConfig>,
}

impl ConfigStore {
    pub fn new() -> Self {
        let path = config_file_path();
        let config = load_or_default(&path);
        Self {
            path,
            config: Mutex::new(config),
        }
    }

    pub fn config_file_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn save_manual_override(&self, runtime_kind: &str, executable_path: &str) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        cfg.manual_overrides
            .insert(runtime_kind.to_string(), executable_path.to_string());
        persist(&self.path, &cfg)?;
        Ok(())
    }

    pub fn get_manual_override(&self, runtime_kind: &str) -> Option<String> {
        self.config
            .lock()
            .ok()
            .and_then(|c| c.manual_overrides.get(runtime_kind).cloned())
    }

    pub fn all_manual_overrides(&self) -> HashMap<String, String> {
        self.config
            .lock()
            .map(|c| c.manual_overrides.clone())
            .unwrap_or_default()
    }

    pub fn detection_fingerprint(&self) -> u64 {
        let cfg = match self.config.lock() {
            Ok(cfg) => cfg,
            Err(_) => return 0,
        };
        let mut hasher = DefaultHasher::new();
        let mut keys: Vec<_> = cfg.manual_overrides.keys().collect();
        keys.sort();
        for key in keys {
            key.hash(&mut hasher);
            if let Some(value) = cfg.manual_overrides.get(key) {
                value.hash(&mut hasher);
            }
        }
        let mut hint_keys: Vec<_> = cfg.settings.detection_path_hints.keys().collect();
        hint_keys.sort();
        for key in hint_keys {
            key.hash(&mut hasher);
            if let Some(value) = cfg.settings.detection_path_hints.get(key) {
                value.hash(&mut hasher);
            }
        }
        let mut disabled = cfg.settings.disabled_runtime_kinds.clone();
        disabled.sort();
        for kind in disabled {
            kind.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn list_projects(&self) -> Vec<ProjectInfo> {
        let mut projects = self
            .config
            .lock()
            .map(|cfg| cfg.projects.clone())
            .unwrap_or_default();
        projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        projects
    }

    pub fn upsert_project(&self, root_path: &str, name: Option<&str>) -> Result<ProjectInfo, String> {
        let root = PathBuf::from(root_path);
        if !root.is_dir() {
            return Err("PROJECT_ROOT_INVALID:项目目录不存在".to_string());
        }
        let canonical = fs::canonicalize(&root)
            .map_err(|e| format!("PROJECT_ROOT_INVALID:{}", e))?;
        let normalized = canonical.to_string_lossy().to_string();
        let project_id = project_id_from_path(&normalized);
        let now = Utc::now().to_rfc3339();
        let display_name = name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                canonical
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| normalized.clone())
            });

        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        let project = if let Some(existing) = cfg
            .projects
            .iter_mut()
            .find(|project| project.project_id == project_id)
        {
            existing.name = display_name;
            existing.root_path = normalized;
            existing.updated_at = now;
            existing.clone()
        } else {
            let project = ProjectInfo {
                project_id,
                name: display_name,
                root_path: normalized,
                created_at: now.clone(),
                updated_at: now,
            };
            cfg.projects.push(project.clone());
            project
        };
        persist(&self.path, &cfg)?;
        Ok(project)
    }

    pub fn remove_project(&self, project_id: &str) -> Result<bool, String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        let before = cfg.projects.len();
        cfg.projects
            .retain(|project| project.project_id != project_id);
        let removed = cfg.projects.len() != before;
        if removed {
            cfg.launch_profiles
                .retain(|profile| profile.project_id != project_id);
            persist(&self.path, &cfg)?;
        }
        Ok(removed)
    }

    pub fn upsert_launch_profile(&self, profile: LaunchProfile) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        if let Some(existing) = cfg
            .launch_profiles
            .iter_mut()
            .find(|p| p.profile_id == profile.profile_id)
        {
            *existing = profile;
        } else {
            cfg.launch_profiles.push(profile);
        }
        persist(&self.path, &cfg)?;
        Ok(())
    }

    pub fn remove_launch_profile(&self, profile_id: &str) -> Result<bool, String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        let before = cfg.launch_profiles.len();
        cfg.launch_profiles
            .retain(|profile| profile.profile_id != profile_id);
        let removed = cfg.launch_profiles.len() != before;
        if removed {
            persist(&self.path, &cfg)?;
        }
        Ok(removed)
    }

    pub fn list_profiles_for_project(&self, project_id: &str) -> Vec<LaunchProfile> {
        self.config
            .lock()
            .map(|c| {
                c.launch_profiles
                    .iter()
                    .filter(|p| p.project_id == project_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_profile(&self, profile_id: &str) -> Option<LaunchProfile> {
        self.config
            .lock()
            .ok()
            .and_then(|c| {
                c.launch_profiles
                    .iter()
                    .find(|p| p.profile_id == profile_id)
                    .cloned()
            })
    }

    pub fn add_profile(&self, profile: LaunchProfile) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        cfg.launch_profiles.push(profile);
        persist(&self.path, &cfg)?;
        Ok(())
    }

    pub fn export_json(&self) -> Result<String, String> {
        let cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        serde_json::to_string_pretty(&*cfg).map_err(|e| e.to_string())
    }

    pub fn import_json(&self, data: &str) -> Result<(), String> {
        let parsed: AppConfig = serde_json::from_str(data).map_err(|e| format!("PROFILE_INVALID:{}", e))?;
        validate_import_config(&parsed)?;
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        *cfg = parsed;
        persist(&self.path, &cfg)?;
        Ok(())
    }

    pub fn write_export_to_path(&self, target_path: &str) -> Result<(), String> {
        let data = self.export_json()?;
        let path = PathBuf::from(target_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        fs::write(path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn all_launch_profiles(&self) -> Vec<LaunchProfile> {
        self.config
            .lock()
            .map(|c| c.launch_profiles.clone())
            .unwrap_or_default()
    }

    pub fn get_settings(&self) -> AppSettings {
        self.config
            .lock()
            .map(|c| c.settings.clone())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: AppSettings) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|_| "config lock poisoned".to_string())?;
        cfg.settings = settings;
        persist(&self.path, &cfg)?;
        Ok(())
    }

    pub fn append_audit_event(&self, event: AuditEvent) {
        if let Ok(mut cfg) = self.config.lock() {
            cfg.audit_events.push(event);
            if cfg.audit_events.len() > 200 {
                let drain = cfg.audit_events.len() - 200;
                cfg.audit_events.drain(0..drain);
            }
            let _ = persist(&self.path, &cfg);
        }
    }

    pub fn list_audit_events(&self, limit: usize) -> Vec<AuditEvent> {
        self.config
            .lock()
            .map(|c| {
                let start = c.audit_events.len().saturating_sub(limit);
                c.audit_events[start..].to_vec()
            })
            .unwrap_or_default()
    }

    pub fn push_recent_error(&self, err: RecentError) {
        if let Ok(mut cfg) = self.config.lock() {
            cfg.recent_errors.push(err);
            if cfg.recent_errors.len() > 50 {
                let drain = cfg.recent_errors.len() - 50;
                cfg.recent_errors.drain(0..drain);
            }
            let _ = persist(&self.path, &cfg);
        }
    }

    pub fn list_recent_errors(&self, limit: usize) -> Vec<RecentError> {
        self.config
            .lock()
            .map(|c| {
                let start = c.recent_errors.len().saturating_sub(limit);
                c.recent_errors[start..].to_vec()
            })
            .unwrap_or_default()
    }

    pub fn extra_protected_names(&self) -> Vec<String> {
        self.get_settings().extra_protected_process_names
    }
}

fn validate_import_config(config: &AppConfig) -> Result<(), String> {
    if config.version != CURRENT_CONFIG_VERSION {
        return Err(format!(
            "PROFILE_INVALID:不支持的配置版本 {}，当前版本 {}",
            config.version, CURRENT_CONFIG_VERSION
        ));
    }

    let mut project_ids = HashSet::new();
    for project in &config.projects {
        if project.project_id.trim().is_empty() || !project_ids.insert(project.project_id.clone()) {
            return Err("PROFILE_INVALID:项目 ID 为空或重复".to_string());
        }
    }

    let mut profile_ids = HashSet::new();
    for profile in &config.launch_profiles {
        if profile.profile_id.trim().is_empty() || !profile_ids.insert(profile.profile_id.clone()) {
            return Err("PROFILE_INVALID:启动配置 ID 为空或重复".to_string());
        }
        if !project_ids.contains(&profile.project_id) {
            return Err(format!(
                "PROFILE_INVALID:启动配置 {} 引用了不存在的项目 {}",
                profile.profile_id, profile.project_id
            ));
        }
    }

    Ok(())
}

pub fn open_at(path: PathBuf) -> ConfigStore {
    let config = load_or_default(&path);
    ConfigStore {
        path,
        config: Mutex::new(config),
    }
}

/// 默认：应用同目录下的 `config/config.json`。
/// `tauri dev` 时解析到仓库根目录的 `config/`（可执行文件在 `src-tauri/target/*/debug`）。
pub fn config_file_path() -> PathBuf {
    if let Ok(custom) = std::env::var("XCHEN_CONFIG_FILE") {
        return PathBuf::from(custom);
    }
    config_dir().join("config.json")
}

pub fn config_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("XCHEN_CONFIG_DIR") {
        return PathBuf::from(custom);
    }
    app_base_dir().join("config")
}

fn app_base_dir() -> PathBuf {
    if let Ok(home) = std::env::var("XCHEN_TOOLS_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(project_root) = project_root_from_exe(&exe) {
            return project_root;
        }
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn project_root_from_exe(exe: &Path) -> Option<PathBuf> {
    let mut dir = exe.parent();
    while let Some(d) = dir {
        if d.file_name().is_some_and(|n| n == "debug" || n == "release") {
            if let Some(target) = d.parent() {
                if target.file_name().is_some_and(|n| n == "target") {
                    if let Some(src_tauri) = target.parent() {
                        if src_tauri.file_name().is_some_and(|n| n == "src-tauri") {
                            return src_tauri.parent().map(|p| p.to_path_buf());
                        }
                    }
                }
            }
        }
        dir = d.parent();
    }
    None
}

fn read_config(path: &Path) -> Option<AppConfig> {
    let data = fs::read_to_string(path).ok()?;
    let config = serde_json::from_str::<AppConfig>(&data).ok()?;
    validate_import_config(&config).ok()?;
    Some(config)
}

fn load_or_default(path: &PathBuf) -> AppConfig {
    if let Some(config) = read_config(path) {
        return config;
    }
    if let Some(config) = read_config(&backup_path(path)) {
        return config;
    }
    AppConfig::default()
}

fn persist(path: &PathBuf, config: &AppConfig) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    let backup_current = read_config(path).is_some();
    atomic_write_with_backup(path, &data, backup_current)
}

pub fn project_id_from_path(root: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(root.to_lowercase().as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ProcessRole;
    use tempfile::tempdir;

    fn sample_project(id: &str) -> ProjectInfo {
        ProjectInfo {
            project_id: id.to_string(),
            name: "demo".to_string(),
            root_path: "C:\\demo".to_string(),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            updated_at: "2026-09-15T00:00:00Z".to_string(),
        }
    }

    fn sample_profile(id: &str, project_id: &str) -> LaunchProfile {
        LaunchProfile {
            profile_id: id.to_string(),
            project_id: project_id.to_string(),
            process_role: ProcessRole::Frontend,
            working_directory: "C:\\demo".to_string(),
            command: "npm run dev".to_string(),
            source_candidate_id: None,
            user_modified: true,
            port_hint: None,
        }
    }

    fn write_config(path: &Path, config: &AppConfig) {
        fs::write(
            path,
            serde_json::to_vec_pretty(config).expect("serialize config"),
        )
        .expect("write config");
    }

    #[test]
    fn import_integrity_accepts_valid_project_profile_graph() {
        let mut config = AppConfig::default();
        config.projects.push(sample_project("project-a"));
        config.launch_profiles.push(sample_profile("profile-a", "project-a"));
        assert!(validate_import_config(&config).is_ok());
    }

    #[test]
    fn import_integrity_rejects_orphan_launch_profile() {
        let mut config = AppConfig::default();
        config.launch_profiles.push(sample_profile("profile-a", "missing-project"));
        assert!(validate_import_config(&config).is_err());
    }

    #[test]
    fn import_integrity_rejects_duplicate_ids_and_future_versions() {
        let mut duplicate = AppConfig::default();
        duplicate.projects.push(sample_project("same"));
        duplicate.projects.push(sample_project("same"));
        assert!(validate_import_config(&duplicate).is_err());

        let mut future = AppConfig::default();
        future.version = CURRENT_CONFIG_VERSION + 1;
        assert!(validate_import_config(&future).is_err());
    }

    #[test]
    fn load_falls_back_to_last_good_backup_when_primary_is_corrupt() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        let mut backup = AppConfig::default();
        backup.manual_overrides.insert(
            "node".to_string(),
            "C:\\Tools\\node.exe".to_string(),
        );
        fs::write(&path, "{broken-json").expect("corrupt primary");
        write_config(&backup_path(&path), &backup);

        let store = open_at(path);
        assert_eq!(
            store.get_manual_override("node").as_deref(),
            Some("C:\\Tools\\node.exe")
        );
    }

    #[test]
    fn load_falls_back_to_backup_when_primary_is_semantically_invalid() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");

        let mut invalid_primary = AppConfig::default();
        invalid_primary
            .launch_profiles
            .push(sample_profile("orphan-profile", "missing-project"));
        write_config(&path, &invalid_primary);

        let mut backup = AppConfig::default();
        backup.manual_overrides.insert(
            "java".to_string(),
            "C:\\Tools\\java.exe".to_string(),
        );
        write_config(&backup_path(&path), &backup);

        let store = open_at(path);
        assert_eq!(
            store.get_manual_override("java").as_deref(),
            Some("C:\\Tools\\java.exe")
        );
    }

    #[test]
    fn load_uses_default_when_primary_and_backup_are_semantically_invalid() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");

        let mut invalid_primary = AppConfig::default();
        invalid_primary.version = CURRENT_CONFIG_VERSION + 1;
        write_config(&path, &invalid_primary);

        let mut invalid_backup = AppConfig::default();
        invalid_backup
            .launch_profiles
            .push(sample_profile("orphan-profile", "missing-project"));
        write_config(&backup_path(&path), &invalid_backup);

        let store = open_at(path);
        assert!(store.list_projects().is_empty());
        assert!(store.all_launch_profiles().is_empty());
        assert!(store.all_manual_overrides().is_empty());
        assert_eq!(store.get_settings().theme, AppSettings::default().theme);
    }

    #[test]
    fn load_prefers_valid_primary_over_backup() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");

        let mut primary = AppConfig::default();
        primary.manual_overrides.insert(
            "node".to_string(),
            "C:\\Primary\\node.exe".to_string(),
        );
        write_config(&path, &primary);

        let mut backup = AppConfig::default();
        backup.manual_overrides.insert(
            "node".to_string(),
            "C:\\Backup\\node.exe".to_string(),
        );
        write_config(&backup_path(&path), &backup);

        let store = open_at(path);
        assert_eq!(
            store.get_manual_override("node").as_deref(),
            Some("C:\\Primary\\node.exe")
        );
    }
}
