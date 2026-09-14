use crate::app_state::AppState;
use crate::domain::{OperationResult, ProjectInfo};
use tauri::State;

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
    state.config.upsert_project(&root_path, name.as_deref())
}

#[tauri::command]
pub fn remove_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<OperationResult, String> {
    if state.config.remove_project(&project_id)? {
        Ok(OperationResult::succeeded("项目记录已移除，磁盘文件未删除"))
    } else {
        Ok(OperationResult::succeeded("项目记录不存在或已移除"))
    }
}
