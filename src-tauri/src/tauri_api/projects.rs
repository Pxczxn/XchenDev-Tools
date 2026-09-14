use crate::config_store::project_catalog;
use crate::domain::{OperationResult, ProjectInfo};

#[tauri::command]
pub fn list_projects() -> Vec<ProjectInfo> {
    project_catalog::list_projects()
}

#[tauri::command]
pub fn upsert_project(root_path: String, name: Option<String>) -> Result<ProjectInfo, String> {
    project_catalog::upsert_project(&root_path, name.as_deref())
}

#[tauri::command]
pub fn remove_project(project_id: String) -> Result<OperationResult, String> {
    if project_catalog::remove_project(&project_id)? {
        Ok(OperationResult::succeeded("项目记录已移除，磁盘文件未删除"))
    } else {
        Ok(OperationResult::succeeded("项目记录不存在或已移除"))
    }
}
