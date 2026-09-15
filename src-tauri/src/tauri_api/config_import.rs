use crate::app_state::AppState;
use crate::domain::{LaunchSessionState, OperationResult};
use tauri::State;

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

#[tauri::command]
pub fn import_app_config_safe(
    state: State<'_, AppState>,
    content: String,
) -> Result<OperationResult, String> {
    ensure_no_active_launch_sessions(&state)?;
    state.config.import_json(&content)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[tauri::command]
pub fn import_app_config_from_path_safe(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<OperationResult, String> {
    ensure_no_active_launch_sessions(&state)?;
    let data = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("PROFILE_INVALID:无法读取文件 {}", e))?;
    state.config.import_json(&data)?;
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
}
