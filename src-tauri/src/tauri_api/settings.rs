use crate::app_state::AppState;
use crate::config_transaction::with_config_rollback;
use crate::domain::{AppSettings, OperationResult};
use crate::settings_guard::normalize_settings;
use tauri::State;

#[tauri::command]
pub fn save_app_settings_safe(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<OperationResult, String> {
    let settings = normalize_settings(settings)?;
    with_config_rollback(&state, || state.config.save_settings(settings))?;
    state.invalidate_env_detection_cache();
    Ok(OperationResult::succeeded("设置已保存"))
}
