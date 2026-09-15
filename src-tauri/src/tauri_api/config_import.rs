use crate::app_state::AppState;
use crate::config_store::AppConfig;
use crate::domain::{LaunchSessionState, OperationResult};
use crate::settings_guard::normalize_settings;
use tauri::State;

use super::launch_profiles::launch_lifecycle_lock;

fn lock_launch_lifecycle() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    launch_lifecycle_lock()
        .lock()
        .map_err(|_| "LAUNCH_LIFECYCLE_LOCK_FAILED:启动关系锁失败".to_string())
}

fn ensure_no_active_launch_sessions(state: &AppState) -> Result<(), String> {
    let has_active = state.command_runner.list_sessions().iter().any(|session| {
        matches!(
            session.state,
            LaunchSessionState::Starting
                | LaunchSessionState::Running
                | LaunchSessionState::Stopping
        )
    });
    if has_active {
        return Err(
            "CONFIG_IMPORT_BLOCKED_ACTIVE_SESSIONS:请先停止所有项目运行会话再导入配置"
                .to_string(),
        );
    }
    Ok(())
}

fn normalize_import_content(content: &str) -> Result<String, String> {
    let mut parsed: AppConfig =
        serde_json::from_str(content).map_err(|e| format!("PROFILE_INVALID:{}", e))?;
    parsed.settings = normalize_settings(parsed.settings)?;
    serde_json::to_string_pretty(&parsed).map_err(|e| format!("PROFILE_INVALID:{}", e))
}

#[tauri::command]
pub fn import_app_config_safe(
    state: State<'_, AppState>,
    content: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;
    ensure_no_active_launch_sessions(&state)?;
    let normalized = normalize_import_content(&content)?;
    state.config.import_json(&normalized)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[tauri::command]
pub fn import_app_config_from_path_safe(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;
    ensure_no_active_launch_sessions(&state)?;
    let data = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("PROFILE_INVALID:无法读取文件 {}", e))?;
    let normalized = normalize_import_content(&data)?;
    state.config.import_json(&normalized)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_runner_allows_config_import_guard() {
        let state = AppState::new();
        assert!(ensure_no_active_launch_sessions(&state).is_ok());
    }

    #[test]
    fn import_rejects_invalid_settings() {
        let mut config = AppConfig::default();
        config.settings.log_retention_days = 0;
        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("invalid settings must fail");
        assert!(err.contains("SETTINGS_INVALID"));
    }

    #[test]
    fn import_normalizes_settings_before_store_validation() {
        let mut config = AppConfig::default();
        config.settings.theme = " DARK ".to_string();
        config.settings.disabled_runtime_kinds = vec![" JAVA ".to_string(), "java".to_string()];
        let json = serde_json::to_string(&config).expect("serialize");
        let normalized = normalize_import_content(&json).expect("normalize");
        let parsed: AppConfig = serde_json::from_str(&normalized).expect("deserialize");
        assert_eq!(parsed.settings.theme, "dark");
        assert_eq!(parsed.settings.disabled_runtime_kinds, vec!["java"]);
    }
}
