use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::{AppSettings, AuditEvent, LaunchProfile, ProjectInfo, RecentError};

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
            version: 1,
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
        if parsed.version == 0 {
            return Err("PROFILE_INVALID:配置版本无效".to_string());
        }
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

fn load_or_default(path: &PathBuf) -> AppConfig {
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&data) {
            return cfg;
        }
    }
    AppConfig::default()
}

fn persist(path: &PathBuf, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn project_id_from_path(root: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(root.to_lowercase().as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}
