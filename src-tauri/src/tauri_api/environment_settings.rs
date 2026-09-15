use crate::app_state::AppState;
use crate::config_transaction::with_config_rollback;
use crate::domain::EnvironmentCandidate;
use crate::environment_detector;
use tauri::State;

#[tauri::command]
pub fn save_manual_override_safe(
    state: State<'_, AppState>,
    runtime_kind: String,
    executable_path: String,
) -> Result<EnvironmentCandidate, String> {
    let candidate = with_config_rollback(&state, || {
        environment_detector::validate_and_save_override(
            &state.config,
            &runtime_kind,
            &executable_path,
        )
    })?;
    state.invalidate_env_detection_cache();
    Ok(candidate)
}
