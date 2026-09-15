use crate::app_state::AppState;
use crate::audit_log;
use crate::domain::{
    OperationResult, OperationStatus, WindowsServiceInfo, WindowsServiceStatus,
};
use crate::service_manager;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::State;
use uuid::Uuid;

const SERVICE_CONFIRMATION_TTL_SECS: i64 = 60;
const SERVICE_STATE_TIMEOUT: Duration = Duration::from_secs(20);
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct PendingServiceConfirmation {
    binding_digest: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceControlConfirmation {
    pub confirmation_token: String,
    pub binding_summary: String,
    pub expires_at: String,
}

fn confirmations() -> &'static Mutex<HashMap<String, PendingServiceConfirmation>> {
    static STORE: OnceLock<Mutex<HashMap<String, PendingServiceConfirmation>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn validate_action(action: &str) -> Result<(), String> {
    match action.to_lowercase().as_str() {
        "start" | "stop" | "restart" => Ok(()),
        _ => Err("SERVICE_ACTION_INVALID:不支持的操作".to_string()),
    }
}

fn find_managed_service(state: &AppState, service_name: &str) -> Result<WindowsServiceInfo, String> {
    let settings = state.config.get_settings();
    service_manager::list_managed_services(
        &settings.managed_service_kinds,
        &settings.managed_service_name_hints,
    )?
    .into_iter()
    .find(|service| service.service_name.eq_ignore_ascii_case(service_name))
    .ok_or_else(|| "SERVICE_NOT_FOUND:服务不存在或不在当前管理范围".to_string())
}

fn binding_digest(service: &WindowsServiceInfo, action: &str) -> String {
    let binding = format!(
        "{}|{}|{:?}",
        service.service_name.to_lowercase(),
        action.to_lowercase(),
        service.status
    );
    let mut hasher = Sha256::new();
    hasher.update(binding.as_bytes());
    hex::encode(hasher.finalize())
}

fn query_service_status(service_name: &str) -> Result<WindowsServiceStatus, String> {
    let output = Command::new("sc")
        .args(["query", service_name])
        .output()
        .map_err(|e| format!("SERVICE_QUERY_FAILED:{}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() { stderr } else { stdout };
        return Err(format!("SERVICE_QUERY_FAILED:{}", detail.trim()));
    }

    let text = String::from_utf8_lossy(&output.stdout).to_uppercase();
    if text.contains("STOP_PENDING") {
        Ok(WindowsServiceStatus::Stopping)
    } else if text.contains("START_PENDING") {
        Ok(WindowsServiceStatus::Starting)
    } else if text.contains("RUNNING") {
        Ok(WindowsServiceStatus::Running)
    } else if text.contains("STOPPED") {
        Ok(WindowsServiceStatus::Stopped)
    } else if text.contains("PAUSED") {
        Ok(WindowsServiceStatus::Paused)
    } else {
        Ok(WindowsServiceStatus::Unknown)
    }
}

fn wait_for_status(service_name: &str, target: WindowsServiceStatus) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let current = query_service_status(service_name)?;
        if current == target {
            return Ok(());
        }
        if started.elapsed() >= SERVICE_STATE_TIMEOUT {
            return Err(format!(
                "SERVICE_CONTROL_TIMEOUT:等待服务进入 {:?} 超时，当前 {:?}",
                target, current
            ));
        }
        thread::sleep(SERVICE_POLL_INTERVAL);
    }
}

fn execute_service_action(
    service: &WindowsServiceInfo,
    action: &str,
) -> Result<(), String> {
    match action.to_lowercase().as_str() {
        "start" => {
            if service.status == WindowsServiceStatus::Running {
                return Ok(());
            }
            if service.status != WindowsServiceStatus::Starting {
                service_manager::control_service(&service.service_name, "start")?;
            }
            wait_for_status(&service.service_name, WindowsServiceStatus::Running)
        }
        "stop" => {
            if service.status == WindowsServiceStatus::Stopped {
                return Ok(());
            }
            if service.status != WindowsServiceStatus::Stopping {
                service_manager::control_service(&service.service_name, "stop")?;
            }
            wait_for_status(&service.service_name, WindowsServiceStatus::Stopped)
        }
        "restart" => {
            if service.status != WindowsServiceStatus::Stopped {
                if service.status != WindowsServiceStatus::Stopping {
                    service_manager::control_service(&service.service_name, "stop")?;
                }
                wait_for_status(&service.service_name, WindowsServiceStatus::Stopped)?;
            }
            service_manager::control_service(&service.service_name, "start")?;
            wait_for_status(&service.service_name, WindowsServiceStatus::Running)
        }
        _ => Err("SERVICE_ACTION_INVALID:不支持的操作".to_string()),
    }
}

#[tauri::command]
pub fn issue_service_control_confirmation_safe(
    state: State<'_, AppState>,
    service_name: String,
    action: String,
) -> Result<ServiceControlConfirmation, String> {
    validate_action(&action)?;
    let service = find_managed_service(&state, &service_name)?;
    if !service.can_control {
        return Err("SERVICE_CONTROL_DENIED:当前服务不可控制".to_string());
    }

    let token = Uuid::new_v4().to_string();
    let pending = PendingServiceConfirmation {
        binding_digest: binding_digest(&service, &action),
        created_at: Utc::now(),
    };
    let mut store = confirmations()
        .lock()
        .map_err(|_| "SERVICE_CONFIRMATION_ISSUE_FAILED:确认锁失败".to_string())?;
    store.retain(|_, item| {
        let age = Utc::now()
            .signed_duration_since(item.created_at)
            .num_seconds();
        age >= 0 && age <= SERVICE_CONFIRMATION_TTL_SECS
    });
    store.insert(token.clone(), pending);

    Ok(ServiceControlConfirmation {
        confirmation_token: token,
        binding_summary: format!("{} {} ({:?})", action, service.display_name, service.status),
        expires_at: (Utc::now() + ChronoDuration::seconds(SERVICE_CONFIRMATION_TTL_SECS)).to_rfc3339(),
    })
}

#[tauri::command]
pub fn control_windows_service_safe(
    state: State<'_, AppState>,
    service_name: String,
    action: String,
    confirmation_token: String,
) -> Result<OperationResult, String> {
    validate_action(&action)?;
    let pending = {
        let mut store = confirmations()
            .lock()
            .map_err(|_| "SERVICE_CONFIRMATION_REQUIRED:确认无效".to_string())?;
        let pending = store
            .remove(&confirmation_token)
            .ok_or_else(|| "SERVICE_CONFIRMATION_REQUIRED:确认令牌不存在".to_string())?;
        let age = Utc::now()
            .signed_duration_since(pending.created_at)
            .num_seconds();
        if age < 0 || age > SERVICE_CONFIRMATION_TTL_SECS {
            return Err("SERVICE_CONFIRMATION_REQUIRED:确认令牌无效或已过期".to_string());
        }
        pending
    };

    let service = find_managed_service(&state, &service_name)?;
    if binding_digest(&service, &action) != pending.binding_digest {
        return Err("SERVICE_CONFIRMATION_REQUIRED:服务状态已变化，请重新确认".to_string());
    }
    if !service.can_control {
        return Err("SERVICE_CONTROL_DENIED:当前服务不可控制".to_string());
    }

    let target = format!("service:{}:{}", service_name, action);
    match execute_service_action(&service, &action) {
        Ok(()) => {
            audit_log::record_action(
                &state.config,
                "CONTROL_WINDOWS_SERVICE",
                &target,
                OperationStatus::Succeeded,
                None,
                Some("服务操作已执行".to_string()),
            );
            Ok(OperationResult::succeeded("服务操作已执行"))
        }
        Err(error) => {
            let code = error.split(':').next().unwrap_or("SERVICE_CONTROL_FAILED");
            audit_log::record_action(
                &state.config,
                "CONTROL_WINDOWS_SERVICE",
                &target,
                OperationStatus::Rejected,
                Some(code.to_string()),
                Some(error.clone()),
            );
            audit_log::record_error(&state.config, code, &error, "control_windows_service_safe");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ServiceKind;

    fn service(status: WindowsServiceStatus) -> WindowsServiceInfo {
        WindowsServiceInfo {
            service_name: "MySQL80".to_string(),
            display_name: "MySQL 8".to_string(),
            status,
            kind: ServiceKind::Mysql,
            can_control: true,
            status_reason: None,
        }
    }

    #[test]
    fn binding_changes_with_action_or_status() {
        let running = service(WindowsServiceStatus::Running);
        let stopped = service(WindowsServiceStatus::Stopped);
        assert_ne!(binding_digest(&running, "stop"), binding_digest(&running, "start"));
        assert_ne!(binding_digest(&running, "stop"), binding_digest(&stopped, "stop"));
    }

    #[test]
    fn rejects_unknown_action() {
        assert!(validate_action("start").is_ok());
        assert!(validate_action("stop").is_ok());
        assert!(validate_action("restart").is_ok());
        assert!(validate_action("delete").is_err());
    }

    #[test]
    fn parses_sc_states_without_false_running_match() {
        assert_eq!(
            parse_sc_status("STATE              : 2  START_PENDING"),
            WindowsServiceStatus::Starting
        );
        assert_eq!(
            parse_sc_status("STATE              : 3  STOP_PENDING"),
            WindowsServiceStatus::Stopping
        );
        assert_eq!(
            parse_sc_status("STATE              : 4  RUNNING"),
            WindowsServiceStatus::Running
        );
    }
}

fn parse_sc_status(text: &str) -> WindowsServiceStatus {
    let upper = text.to_uppercase();
    if upper.contains("STOP_PENDING") {
        WindowsServiceStatus::Stopping
    } else if upper.contains("START_PENDING") {
        WindowsServiceStatus::Starting
    } else if upper.contains("RUNNING") {
        WindowsServiceStatus::Running
    } else if upper.contains("STOPPED") {
        WindowsServiceStatus::Stopped
    } else if upper.contains("PAUSED") {
        WindowsServiceStatus::Paused
    } else {
        WindowsServiceStatus::Unknown
    }
}
