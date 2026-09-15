use crate::app_state::AppState;
use crate::config_transaction::with_config_rollback;
use crate::domain::{LaunchSessionState, OperationResult, ProjectInfo};
use std::collections::HashSet;
use tauri::State;

use super::launch_profiles::launch_lifecycle_lock;

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Vec<ProjectInfo> {
    state.config.list_projects()
}

#[tauri::command]
pub fn upsert_project(
    state: State<'_, AppState>,
    root_path: String,
    name: Option<String>,
) -> Result<ProjectInfo, String> {
    with_config_rollback(&state, || {
        state.config.upsert_project(&root_path, name.as_deref())
    })
}

#[tauri::command]
pub fn remove_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = launch_lifecycle_lock()
        .lock()
        .map_err(|_| "LAUNCH_LIFECYCLE_LOCK_FAILED:启动关系锁失败".to_string())?;

    let profile_ids: HashSet<String> = state
        .config
        .list_profiles_for_project(&project_id)
        .into_iter()
        .map(|profile| profile.profile_id)
        .collect();

    let has_active_session = state.command_runner.list_sessions().iter().any(|session| {
        profile_ids.contains(&session.profile_id)
            && matches!(
                session.state,
                LaunchSessionState::Starting
                    | LaunchSessionState::Running
                    | LaunchSessionState::Stopping
            )
    });
    if has_active_session {
        return Err("PROJECT_RUNNING:请先停止该项目的运行会话".to_string());
    }

    let removed = with_config_rollback(&state, || state.config.remove_project(&project_id))?;
    if removed {
        Ok(OperationResult::succeeded(
            "项目记录与启动配置已移除，磁盘文件未删除",
        ))
    } else {
        Ok(OperationResult::succeeded("项目记录不存在或已移除"))
    }
}
