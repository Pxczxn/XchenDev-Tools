use crate::app_state::{AppState, PROCESS_CONFIRMATION_TTL_SECS};
use crate::audit_log;
use crate::domain::{OperationResult, OperationStatus};
use crate::port_manager;
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

fn execute_termination(
    state: &AppState,
    pid: u32,
    force: bool,
    action: &str,
    target: &str,
    error_source: &str,
) -> Result<OperationResult, String> {
    match process_manager::terminate_pid_with_extra(
        pid,
        force,
        &state.config.extra_protected_names(),
    ) {
        Ok(()) => {
            audit_log::record_action(
                &state.config,
                action,
                target,
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
                action,
                target,
                OperationStatus::Rejected,
                Some(code.to_string()),
                Some(error.clone()),
            );
            audit_log::record_error(&state.config, code, &error, error_source);
            Err(error)
        }
    }
}

fn record_rejected_snapshot(
    state: &AppState,
    action: &str,
    target: &str,
    reason_code: &str,
    message: &str,
) -> OperationResult {
    audit_log::record_action(
        &state.config,
        action,
        target,
        OperationStatus::Rejected,
        Some(reason_code.to_string()),
        Some(message.to_string()),
    );
    OperationResult::rejected(reason_code, message)
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

    let _identity_guard = process_manager::pin_process_identity(pid)?;

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
    let protection =
        process_manager::protection_with_extra(&summary, &state.config.extra_protected_names());
    if protection.is_protected {
        return Err("PROCESS_PROTECTED:目标进程受保护".to_string());
    }

    let (token, binding_summary) =
        state.issue_process_confirmation(pid, &expected_name, expected_cwd.as_deref(), &mode)?;
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
    execute_termination(
        &state,
        pid,
        force,
        "TERMINATE_PROCESS",
        &target,
        "terminate_process_safe",
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Flat Tauri IPC contract; grouping would break the frontend command payload.
pub fn terminate_port_process_safe(
    state: State<'_, AppState>,
    pid: u32,
    protocol: String,
    port: u16,
    snapshot_digest: String,
    mode: String,
    confirmation_token: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<OperationResult, String> {
    validate_mode(&mode)?;
    let protocol = protocol.trim().to_lowercase();
    let target = format!("{}:{}:pid:{}:{}", protocol, port, pid, expected_name);

    let _identity_guard = process_manager::pin_process_identity(pid)?;
    state.consume_process_confirmation(
        &confirmation_token,
        pid,
        &expected_name,
        expected_cwd.as_deref(),
        &mode,
    )?;
    process_manager::verify_process_snapshot(pid, &expected_name, expected_cwd.as_deref())?;

    let current = process_manager::find_process_summary(pid)
        .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
    let current_digest =
        process_manager::digest_for(pid, &current.name, current.working_directory.as_deref());
    if current_digest != snapshot_digest {
        return Ok(record_rejected_snapshot(
            &state,
            "TERMINATE_PORT_PROCESS",
            &target,
            "PROCESS_SNAPSHOT_MISMATCH",
            "端口进程快照已失效，请刷新",
        ));
    }

    if !port_manager::pid_owns_port(&protocol, port, pid)? {
        return Ok(record_rejected_snapshot(
            &state,
            "TERMINATE_PORT_PROCESS",
            &target,
            "PORT_OWNERSHIP_CHANGED",
            "端口归属已变化，请刷新",
        ));
    }

    let force = mode.eq_ignore_ascii_case("force");
    execute_termination(
        &state,
        pid,
        force,
        "TERMINATE_PORT_PROCESS",
        &target,
        "terminate_port_process_safe",
    )
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
    validate_mode(&mode)?;
    let target = format!("pid:{}:{}:directory", pid, expected_name);

    let _identity_guard = match process_manager::pin_process_identity(pid) {
        Ok(guard) => guard,
        Err(error) if error.starts_with("PROCESS_NOT_FOUND:") => {
            return Ok(record_rejected_snapshot(
                &state,
                "TERMINATE_DIRECTORY_PROCESS",
                &target,
                "PROCESS_SNAPSHOT_MISMATCH",
                "进程快照已失效，请刷新",
            ));
        }
        Err(error) => return Err(error),
    };

    state.consume_process_confirmation(
        &confirmation_token,
        pid,
        &expected_name,
        expected_cwd.as_deref(),
        &mode,
    )?;
    process_manager::verify_process_snapshot(pid, &expected_name, expected_cwd.as_deref())?;

    let current = match process_manager::find_process_summary(pid) {
        Some(summary) => summary,
        None => {
            return Ok(record_rejected_snapshot(
                &state,
                "TERMINATE_DIRECTORY_PROCESS",
                &target,
                "PROCESS_SNAPSHOT_MISMATCH",
                "进程快照已失效，请刷新",
            ));
        }
    };
    let current_digest =
        process_manager::digest_for(pid, &current.name, current.working_directory.as_deref());
    if current_digest != snapshot_digest {
        return Ok(record_rejected_snapshot(
            &state,
            "TERMINATE_DIRECTORY_PROCESS",
            &target,
            "PROCESS_SNAPSHOT_MISMATCH",
            "进程快照已失效，请刷新",
        ));
    }

    let force = mode.eq_ignore_ascii_case("force");
    execute_termination(
        &state,
        pid,
        force,
        "TERMINATE_DIRECTORY_PROCESS",
        &target,
        "terminate_directory_process_safe",
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
