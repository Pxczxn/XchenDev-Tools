use crate::domain::ProjectInfo;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::{config_dir, project_id_from_path};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectCatalog {
    version: u32,
    #[serde(default)]
    projects: Vec<ProjectInfo>,
}

impl Default for ProjectCatalog {
    fn default() -> Self {
        Self {
            version: 1,
            projects: Vec::new(),
        }
    }
}

fn catalog_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn catalog_path() -> PathBuf {
    config_dir().join("projects.json")
}

fn load(path: &Path) -> ProjectCatalog {
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str::<ProjectCatalog>(&data).ok())
        .unwrap_or_default()
}

fn persist(path: &Path, catalog: &ProjectCatalog) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(catalog).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, data).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn normalize_root_path(root_path: &str) -> Result<String, String> {
    let path = PathBuf::from(root_path);
    if !path.is_dir() {
        return Err("PROJECT_ROOT_INVALID:项目目录不存在".to_string());
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|e| format!("PROJECT_ROOT_INVALID:{}", e))?;
    let mut value = canonical.to_string_lossy().to_string();
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    Ok(value)
}

fn default_name(root_path: &str) -> String {
    Path::new(root_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| root_path.to_string())
}

pub fn list_projects() -> Vec<ProjectInfo> {
    let _guard = match catalog_lock().lock() {
        Ok(guard) => guard,
        Err(_) => return Vec::new(),
    };
    let mut projects = load(&catalog_path()).projects;
    projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    projects
}

pub fn upsert_project(root_path: &str, name: Option<&str>) -> Result<ProjectInfo, String> {
    let _guard = catalog_lock()
        .lock()
        .map_err(|_| "project catalog lock poisoned".to_string())?;
    let normalized = normalize_root_path(root_path)?;
    let project_id = project_id_from_path(&normalized);
    let now = Utc::now().to_rfc3339();
    let display_name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_name(&normalized));

    let path = catalog_path();
    let mut catalog = load(&path);
    let project = if let Some(existing) = catalog
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
        catalog.projects.push(project.clone());
        project
    };
    persist(&path, &catalog)?;
    Ok(project)
}

pub fn remove_project(project_id: &str) -> Result<bool, String> {
    let _guard = catalog_lock()
        .lock()
        .map_err(|_| "project catalog lock poisoned".to_string())?;
    let path = catalog_path();
    let mut catalog = load(&path);
    let before = catalog.projects.len();
    catalog
        .projects
        .retain(|project| project.project_id != project_id);
    let removed = catalog.projects.len() != before;
    if removed {
        persist(&path, &catalog)?;
    }
    Ok(removed)
}
