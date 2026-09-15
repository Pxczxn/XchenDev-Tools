use crate::app_state::AppState;
use crate::config_store::project_id_from_path;
use crate::domain::{
    AppSettings, DirectoryProcessMatch, HealthCheckResponse, LaunchProfile, OperationResult,
    ProjectScanResult, WindowsServiceInfo,
};
use crate::environment_detector;
use crate::port_manager;
use crate::process_manager;
use crate::project_scanner;
use crate::security_guard;
use crate::service_manager;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use tauri::State;

#[tauri::command]
pub fn health_check() -> Result<HealthCheckResponse, crate::error::AppError> {
    if std::env::var("XCHEN_IPC_SIMULATE_FAILURE").as_deref() == Ok("1") {
        return Err(crate::error::AppError::IpcNotReady);
    }
    Ok(HealthCheckResponse {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        ipc_status: "ONLINE".to_string(),
    })
}

#[tauri::command]
pub fn list_environment_candidates(
    state: State<'_, AppState>,
    runtime_kinds: Vec<String>,
    force_refresh: Option<bool>,
) -> Result<Vec<crate::domain::EnvironmentCandidate>, String> {
    let force_refresh = force_refresh.unwrap_or(false);
    if let Some(cached) = state.cached_environment_candidates(&runtime_kinds, force_refresh)? {
        return Ok(cached);
    }
    let candidates = environment_detector::list_candidates(&state.config, &runtime_kinds)?;
    state.store_environment_candidates(&runtime_kinds, candidates.clone())?;
    Ok(candidates)
}

#[tauri::command]
pub fn inspect_port(
    state: State<'_, AppState>,
    protocol: String,
    port: u16,
) -> Result<Vec<crate::domain::PortOccupancy>, String> {
    let extra = state.config.extra_protected_names();
    let mut rows = port_manager::inspect_port(&protocol, port)?;
    for row in &mut rows {
        row.protection = process_manager::protection_with_extra(&row.process, &extra);
    }
    Ok(rows)
}

#[tauri::command]
pub fn inspect_directory_processes(
    state: State<'_, AppState>,
    root_path: String,
) -> Result<Vec<DirectoryProcessMatch>, String> {
    let extra = state.config.extra_protected_names();
    let root = security_guard::normalize_directory(&root_path)?;
    let summaries = process_manager::list_all_summaries();
    let mut matches = Vec::new();
    for summary in summaries {
        let cwd = match &summary.working_directory {
            Some(c) => c,
            None => continue,
        };
        let level = security_guard::directory_matches_prefix(cwd, &root);
        if level.is_none() {
            continue;
        }
        let ports = port_manager::ports_for_pid(summary.pid);
        let digest = process_manager::digest_for(
            summary.pid,
            &summary.name,
            summary.working_directory.as_deref(),
        );
        let protection = process_manager::protection_with_extra(&summary, &extra);
        matches.push(DirectoryProcessMatch {
            match_level: level.unwrap(),
            pid: summary.pid,
            parent_pid: None,
            name: summary.name,
            working_directory: summary.working_directory,
            command_line: summary.command_line,
            ports,
            protection,
            snapshot_digest: digest,
        });
    }
    Ok(matches)
}

#[tauri::command]
pub fn scan_project_directory(root_path: String) -> Result<ProjectScanResult, String> {
    project_scanner::scan_project_directory(&root_path)
}

#[tauri::command]
pub fn list_launch_profiles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<LaunchProfile>, String> {
    Ok(state.config.list_profiles_for_project(&project_id))
}

#[tauri::command]
pub fn stop_launch_session(
    state: State<'_, AppState>,
    launch_session_id: String,
) -> Result<OperationResult, String> {
    state.command_runner.stop(&launch_session_id)?;
    Ok(OperationResult::succeeded("会话已停止"))
}

#[tauri::command]
pub fn project_id_for_path(root_path: String) -> String {
    project_id_from_path(&root_path)
}

#[tauri::command]
pub fn export_app_config(state: State<'_, AppState>) -> Result<String, String> {
    state.config.export_json()
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("CONFIG_EXPORT_TARGET_INVALID:{}", e))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn resolved_path_for_compare(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return std::fs::canonicalize(path)
            .map_err(|e| format!("CONFIG_EXPORT_TARGET_INVALID:{}", e));
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
        if parent.exists() {
            let canonical_parent = std::fs::canonicalize(parent)
                .map_err(|e| format!("CONFIG_EXPORT_TARGET_INVALID:{}", e))?;
            return Ok(canonical_parent.join(file_name));
        }
    }
    lexical_absolute(path)
}

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

fn ensure_export_target_safe(config_path: &Path, target_path: &str) -> Result<(), String> {
    let target_path = target_path.trim();
    if target_path.is_empty() {
        return Err("CONFIG_EXPORT_TARGET_INVALID:导出路径不能为空".to_string());
    }
    let target = PathBuf::from(target_path);
    if target.file_name().is_none() {
        return Err("CONFIG_EXPORT_TARGET_INVALID:导出目标必须是文件".to_string());
    }

    let target = resolved_path_for_compare(&target)?;
    let protected = [
        config_path.to_path_buf(),
        appended_path(config_path, ".bak"),
        appended_path(config_path, ".tmp"),
    ];
    for path in protected {
        let resolved = resolved_path_for_compare(&path)?;
        if same_path(&target, &resolved) {
            return Err(
                "CONFIG_EXPORT_TARGET_PROTECTED:导出目标不能覆盖当前配置或其事务文件"
                    .to_string(),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub fn export_app_config_to_path(
    state: State<'_, AppState>,
    target_path: String,
) -> Result<OperationResult, String> {
    ensure_export_target_safe(&state.config.config_file_path(), &target_path)?;
    state.config.write_export_to_path(&target_path)?;
    Ok(OperationResult::succeeded("配置已导出"))
}

#[tauri::command]
pub fn get_config_paths(state: State<'_, AppState>) -> (String, String) {
    let file = state.config.config_file_path();
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    (dir, file.to_string_lossy().to_string())
}

#[tauri::command]
pub fn list_managed_services(state: State<'_, AppState>) -> Result<Vec<WindowsServiceInfo>, String> {
    let settings = state.config.get_settings();
    service_manager::list_managed_services(
        &settings.managed_service_kinds,
        &settings.managed_service_name_hints,
    )
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> AppSettings {
    state.config.get_settings()
}

#[tauri::command]
pub fn list_default_protected_processes() -> Vec<String> {
    security_guard::default_protected_process_names()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn export_target_rejects_managed_config_files() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("config.json");
        std::fs::write(&config, "{}").expect("config");

        assert!(ensure_export_target_safe(&config, config.to_str().expect("config path")).is_err());
        assert!(ensure_export_target_safe(
            &config,
            appended_path(&config, ".bak").to_str().expect("backup path")
        )
        .is_err());
        assert!(ensure_export_target_safe(
            &config,
            appended_path(&config, ".tmp").to_str().expect("temp path")
        )
        .is_err());
    }

    #[test]
    fn export_target_allows_normal_sibling_file() {
        let dir = tempdir().expect("tempdir");
        let config = dir.path().join("config.json");
        std::fs::write(&config, "{}").expect("config");
        let export = dir.path().join("backup-export.json");

        assert!(ensure_export_target_safe(&config, export.to_str().expect("export path")).is_ok());
    }

    #[test]
    fn export_target_rejects_alias_with_parent_navigation() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).expect("nested");
        let config = dir.path().join("config.json");
        std::fs::write(&config, "{}").expect("config");
        let alias = nested.join("..").join("config.json");

        assert!(ensure_export_target_safe(&config, alias.to_str().expect("alias path")).is_err());
    }
}
