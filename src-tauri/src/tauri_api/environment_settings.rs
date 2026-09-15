use crate::app_state::AppState;
use crate::config_transaction::with_config_rollback;
use crate::domain::EnvironmentCandidate;
use crate::environment_detector;
use tauri::State;

fn normalize_runtime_kind(runtime_kind: &str) -> Result<String, String> {
    let runtime_kind = runtime_kind.trim().to_lowercase();
    if environment_detector::all_runtime_kind_ids()
        .iter()
        .any(|allowed| *allowed == runtime_kind)
    {
        Ok(runtime_kind)
    } else {
        Err(format!(
            "RUNTIME_KIND_INVALID:不支持的运行时类型 {}",
            runtime_kind
        ))
    }
}

#[tauri::command]
pub fn save_manual_override_safe(
    state: State<'_, AppState>,
    runtime_kind: String,
    executable_path: String,
) -> Result<EnvironmentCandidate, String> {
    let runtime_kind = normalize_runtime_kind(&runtime_kind)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_kind_is_normalized_and_known_values_are_allowed() {
        assert_eq!(normalize_runtime_kind(" JAVA ").unwrap(), "java");
        assert_eq!(normalize_runtime_kind("Rust").unwrap(), "rust");
    }

    #[test]
    fn unknown_runtime_kind_is_rejected() {
        let err = normalize_runtime_kind("go").expect_err("unknown runtime must fail");
        assert!(err.contains("RUNTIME_KIND_INVALID"));
    }
}
