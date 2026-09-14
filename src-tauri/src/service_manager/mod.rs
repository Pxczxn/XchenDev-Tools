use crate::domain::{ServiceKind, WindowsServiceInfo, WindowsServiceStatus};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct PsServiceRow {
    Name: String,
    DisplayName: String,
    Status: String,
}

pub fn normalize_managed_service_kinds(kinds: &[String]) -> Vec<String> {
    let allowed: HashSet<&str> = ["mysql", "redis"].into_iter().collect();
    let mut out: Vec<String> = kinds
        .iter()
        .map(|k| k.trim().to_lowercase())
        .filter(|k| allowed.contains(k.as_str()))
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn discovery_pattern(enabled_kinds: &[String]) -> Option<String> {
    let set: HashSet<String> = normalize_managed_service_kinds(enabled_kinds)
        .into_iter()
        .collect();
    let mut parts: Vec<&str> = Vec::new();
    if set.contains("mysql") {
        parts.push("mysql|mariadb");
    }
    if set.contains("redis") {
        parts.push("redis");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("|"))
    }
}

pub fn list_managed_services(
    enabled_kinds: &[String],
    name_hints: &HashMap<String, String>,
) -> Result<Vec<WindowsServiceInfo>, String> {
    let enabled = normalize_managed_service_kinds(enabled_kinds);
    let enabled_set: HashSet<String> = enabled.iter().cloned().collect();
    if enabled_set.is_empty() {
        return Ok(vec![]);
    }

    let mut result = Vec::new();
    if let Some(pattern) = discovery_pattern(enabled_kinds) {
        for row in query_by_pattern(&pattern)? {
            let info = map_row(row);
            if service_kind_enabled(&info.kind, &enabled_set) {
                push_unique_service(&mut result, info);
            }
        }
    }

    for kind in &enabled {
        let hint = name_hints
            .get(kind)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        if let Some(service_name) = hint {
            if result
                .iter()
                .any(|r| r.service_name.eq_ignore_ascii_case(service_name))
            {
                continue;
            }
            if let Some(row) = query_service_by_name(service_name)? {
                let mut info = map_row(row);
                info.kind = kind_from_id(kind);
                if service_kind_enabled(&info.kind, &enabled_set) {
                    push_unique_service(&mut result, info);
                }
            }
        }
    }

    result.sort_by(|a, b| {
        kind_sort_key(&a.kind)
            .cmp(&kind_sort_key(&b.kind))
            .then(a.service_name.to_lowercase().cmp(&b.service_name.to_lowercase()))
    });
    Ok(result)
}

fn kind_sort_key(kind: &ServiceKind) -> u8 {
    match kind {
        ServiceKind::Mysql => 0,
        ServiceKind::Redis => 1,
        ServiceKind::Unknown => 2,
    }
}

fn kind_from_id(kind: &str) -> ServiceKind {
    match kind.to_lowercase().as_str() {
        "mysql" => ServiceKind::Mysql,
        "redis" => ServiceKind::Redis,
        _ => ServiceKind::Unknown,
    }
}

fn push_unique_service(list: &mut Vec<WindowsServiceInfo>, item: WindowsServiceInfo) {
    if list
        .iter()
        .any(|r| r.service_name.eq_ignore_ascii_case(&item.service_name))
    {
        return;
    }
    list.push(item);
}

fn query_by_pattern(pattern: &str) -> Result<Vec<PsServiceRow>, String> {
    let script = format!(
        "Get-Service | Where-Object {{ $_.Name -match '{0}' -or $_.DisplayName -match '{0}' }} | Select-Object Name, DisplayName, @{{N='Status';E={{$_.Status.ToString()}}}} | ConvertTo-Json -Compress",
        pattern
    );
    run_service_query_script(&script)
}

fn query_service_by_name(service_name: &str) -> Result<Option<PsServiceRow>, String> {
    let escaped = service_name.replace('\'', "''");
    let script = format!(
        "$s = Get-Service -Name '{0}' -ErrorAction SilentlyContinue; if ($null -eq $s) {{ exit 0 }}; $s | Select-Object Name, DisplayName, @{{N='Status';E={{$_.Status.ToString()}}}} | ConvertTo-Json -Compress",
        escaped
    );
    let rows = run_service_query_script(&script)?;
    Ok(rows.into_iter().next())
}

fn run_service_query_script(script: &str) -> Result<Vec<PsServiceRow>, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("SERVICE_QUERY_FAILED:{}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SERVICE_QUERY_FAILED:{}", err));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = stdout.trim();
    if text.is_empty() {
        return Ok(vec![]);
    }
    if text.starts_with('[') {
        serde_json::from_str(text).map_err(|e| format!("SERVICE_QUERY_FAILED:{}", e))
    } else {
        let one: PsServiceRow =
            serde_json::from_str(text).map_err(|e| format!("SERVICE_QUERY_FAILED:{}", e))?;
        Ok(vec![one])
    }
}

fn service_kind_enabled(kind: &ServiceKind, enabled: &HashSet<String>) -> bool {
    match kind {
        ServiceKind::Mysql => enabled.contains("mysql"),
        ServiceKind::Redis => enabled.contains("redis"),
        ServiceKind::Unknown => false,
    }
}

fn map_row(row: PsServiceRow) -> WindowsServiceInfo {
    let kind = classify_service(&row.Name, &row.DisplayName);
    let status = parse_status(&row.Status);
    WindowsServiceInfo {
        service_name: row.Name,
        display_name: row.DisplayName,
        status,
        kind,
        can_control: true,
        status_reason: None,
    }
}

fn classify_service(name: &str, display: &str) -> ServiceKind {
    let n = name.to_lowercase();
    let d = display.to_lowercase();
    if n.contains("mysql") || n.contains("mariadb") || d.contains("mysql") || d.contains("mariadb") {
        return ServiceKind::Mysql;
    }
    if n.contains("redis") || d.contains("redis") {
        return ServiceKind::Redis;
    }
    ServiceKind::Unknown
}

fn parse_status(raw: &str) -> WindowsServiceStatus {
    match raw.to_lowercase().as_str() {
        "running" => WindowsServiceStatus::Running,
        "stopped" => WindowsServiceStatus::Stopped,
        "startpending" => WindowsServiceStatus::Starting,
        "stoppending" => WindowsServiceStatus::Stopping,
        "paused" => WindowsServiceStatus::Paused,
        _ => WindowsServiceStatus::Unknown,
    }
}

pub fn control_service(service_name: &str, action: &str) -> Result<(), String> {
    let action_lower = action.to_lowercase();
    if action_lower == "restart" {
        let _ = run_sc(&["stop", service_name]);
        return run_sc(&["start", service_name]);
    }
    let args: [&str; 2] = match action_lower.as_str() {
        "start" => ["start", service_name],
        "stop" => ["stop", service_name],
        _ => return Err("SERVICE_ACTION_INVALID:不支持的操作".to_string()),
    };
    run_sc(&args)
}

fn run_sc(args: &[&str]) -> Result<(), String> {
    let output = Command::new("sc")
        .args(args)
        .output()
        .map_err(|e| format!("SERVICE_CONTROL_FAILED:{}", e))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let msg = if !stderr.trim().is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };
    if msg.to_lowercase().contains("access is denied") {
        return Err("SERVICE_CONTROL_DENIED:权限不足，请以管理员身份运行".to_string());
    }
    Err(format!("SERVICE_CONTROL_FAILED:{}", msg.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_pattern_respects_enabled_kinds() {
        assert_eq!(
            discovery_pattern(&["redis".to_string()]),
            Some("redis".to_string())
        );
        assert_eq!(
            discovery_pattern(&["mysql".to_string()]),
            Some("mysql|mariadb".to_string())
        );
        assert_eq!(
            discovery_pattern(&["mysql".to_string(), "redis".to_string()]),
            Some("mysql|mariadb|redis".to_string())
        );
    }

    #[test]
    fn empty_kinds_skips_discovery() {
        assert_eq!(discovery_pattern(&[]), None);
    }
}

pub fn service_status_to_runtime_state(status: &WindowsServiceStatus) -> crate::domain::RuntimeItemState {
    use crate::domain::RuntimeItemState;
    match status {
        WindowsServiceStatus::Running => RuntimeItemState::Running,
        WindowsServiceStatus::Stopped => RuntimeItemState::Stopped,
        WindowsServiceStatus::Starting => RuntimeItemState::Starting,
        WindowsServiceStatus::Stopping => RuntimeItemState::Stopping,
        WindowsServiceStatus::Paused | WindowsServiceStatus::Unknown => RuntimeItemState::Unknown,
    }
}
