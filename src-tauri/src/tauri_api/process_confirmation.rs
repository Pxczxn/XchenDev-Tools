use crate::app_state::{AppState, PROCESS_CONFIRMATION_TTL_SECS};
use crate::audit_log;
use crate::domain::{OperationResult, OperationStatus};
use crate::process_manager;
use chrono::{Duration, Utc};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct ProcessTerminationConfirmation {
    pub confirmation_token: String,
    pub binding_summary: String,
    pub expires_at: String,
}

fn validate_mode(mode: &str) -> Result<(), String> {
    if mode.eq_ignore_ascii_case("normal") || mode.eq_ignore_ascii_case("force") {
        Ok(())
    } else {
        Err("TERMINATE_MODE_INVALID:无效终止模式".to_string())
    }
}

#[tauri::command]
pub fn issue_process_termination_confirmation_safe(
    state: State<'_, AppState>,
    pid: u32,
    mode: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<ProcessTerminationConfirmation, String> {
    validate_mode(&mode)?;
    let summary = process_manager::find_process_summary(pid)
        .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
    if !summary.name.eq_ignore_ascii_case(&expected_name) {
        return Err("PROCESS_SNAPSHOT_MISMATCH:进程名不匹配".to_string());
    }
    if let Some(expected) = expected_cwd.as_deref() {
        match summary.working_directory.as_deref() {
            Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
            _ => return Err("PROCESS_SNAPSHOT_MISMATCH:工作目录不匹配".to_string()),
        }
    }
    let protection = process_manager::protection_with_extra(
        &summary,
        &state.config.extra_protected_names(),
    );
    if protection.is_protected {
        return Err("PROCESS_PROTECTED:目标进程受保护".to_string());
    }

    let (token, binding_summary) = state.issue_process_confirmation(
        pid,
        &expected_name,
        expected_cwd.as_deref(),
        &mode,
    )?;
    Ok(ProcessTerminationConfirmation {
        confirmation_token: token,
        binding_summary,
        expires_at: (Utc::now() + Duration::seconds(PROCESS_CONFIRMATION_TTL_SECS)).to_rfc3339(),
    })
}

#[tauri::command]
pub fn terminate_process_safe(
    state: State<'_, AppState>,
    pid: u32,
    mode: String,
    confirmation_token: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<OperationResult, String> {
    validate_mode(&mode)?;

    // Pin the current Windows process object before consuming the confirmation token.
    // If the original process has already exited and the PID was reused, the token's
    // creation-time binding below rejects the replacement. If it exits after this point,
    // keeping the handle alive prevents the PID from being reused until this operation ends.
    let _identity_guard = process_manager::pin_process_identity(pid)?;

    state.consume_process_confirmation(
        &confirmation_token,
        pid,
        &expected_name,
        expected_cwd.as_deref(),
        &mode,
    )?;
    process_manager::verify_process_snapshot(pid, &expected_name, expected_cwd.as_deref())?;

    let force = mode.eq_ignore_ascii_case("force");
    let target = format!("pid:{}:{}", pid, expected_name);
    match process_manager::terminate_pid_with_extra(
        pid,
        force,
        &state.config.extra_protected_names(),
    ) {
        Ok(()) => {
            audit_log::record_action(
                &state.config,
                "TERMINATE_PROCESS",
                &target,
                OperationStatus::Succeeded,
                None,
                Some("进程已终止".to_string()),
            );
            Ok(OperationResult::succeeded("进程已终止"))
        }
        Err(error) => {
            let code = error.split(':').next().unwrap_or("TERMINATE_FAILED");
            audit_log::record_action(
                &state.config,
                "TERMINATE_PROCESS",
                &target,
                OperationStatus::Rejected,
                Some(code.to_string()),
                Some(error.clone()),
            );
            audit_log::record_error(&state.config, code, &error, "terminate_process_safe");
            Err(error)
        }
    }
}

#[tauri::command]
pub fn terminate_directory_process_safe(
    state: State<'_, AppState>,
    pid: u32,
    snapshot_digest: String,
    mode: String,
    confirmation_token: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<OperationResult, String> {
    let current_digest = process_manager::digest_for(
        pid,
        &expected_name,
        expected_cwd.as_deref(),
    );
    if current_digest != snapshot_digest {
        return Ok(OperationResult::rejected(
            "PROCESS_SNAPSHOT_MISMATCH",
            "进程快照已失效，请刷新",
        ));
    }

    terminate_process_safe(
        state,
        pid,
        mode,
        confirmation_token,
        expected_name,
        expected_cwd,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_validation_rejects_unknown_values() {
        assert!(validate_mode("normal").is_ok());
        assert!(validate_mode("force").is_ok());
        assert!(validate_mode("anything-else").is_err());
    }
}
