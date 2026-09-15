use crate::app_state::AppState;
use crate::config_store::AppConfig;
use crate::config_transaction::with_config_rollback;
use crate::domain::{AppSettings, OperationResult};
use crate::history_retention::prune_config_history;
use crate::settings_guard::normalize_settings;
use chrono::Utc;
use tauri::State;

#[tauri::command]
pub fn save_app_settings_safe(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<OperationResult, String> {
    let settings = normalize_settings(settings)?;
    with_config_rollback(&state, || {
        let current = state.config.export_json()?;
        let mut config: AppConfig =
            serde_json::from_str(&current).map_err(|e| format!("PROFILE_INVALID:{}", e))?;
        config.settings = settings;
        prune_config_history(&mut config, Utc::now());
        let normalized =
            serde_json::to_string_pretty(&config).map_err(|e| format!("PROFILE_INVALID:{}", e))?;
        state.config.import_json(&normalized)
    })?;
    state.invalidate_env_detection_cache();
    Ok(OperationResult::succeeded("设置已保存"))
}
