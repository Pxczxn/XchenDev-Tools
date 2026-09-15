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
    let runtime_kind = runtime_kind.trim().to_lowercase();
    if !environment_detector::all_runtime_kind_ids()
        .iter()
        .any(|allowed| *allowed == runtime_kind)
    {
        return Err(format!(
            "RUNTIME_KIND_INVALID:不支持的运行时类型 {}",
            runtime_kind
        ));
    }

    let executable_path = executable_path.trim().to_string();
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
