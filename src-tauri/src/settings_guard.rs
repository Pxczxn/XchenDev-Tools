use crate::domain::AppSettings;
use std::collections::{HashMap, HashSet};

const MAX_LOG_RETENTION_DAYS: u32 = 365;
const MAX_PROTECTED_NAMES: usize = 128;
const MAX_HINTS: usize = 32;
const MAX_VALUE_LEN: usize = 512;
const RUNTIME_KINDS: &[&str] = &["java", "python", "node", "php", "rust"];
const MANAGED_SERVICE_KINDS: &[&str] = &["mysql", "redis"];

fn invalid(message: impl Into<String>) -> String {
    format!("SETTINGS_INVALID:{}", message.into())
}

fn clean_text(value: &str, field: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(format!("{} 不能为空", field)));
    }
    if trimmed.len() > MAX_VALUE_LEN {
        return Err(invalid(format!("{} 过长", field)));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(invalid(format!("{} 包含控制字符", field)));
    }
    Ok(trimmed.to_string())
}

fn normalize_kind_list(
    values: &[String],
    allowed: &[&str],
    field: &str,
) -> Result<Vec<String>, String> {
    let allowed: HashSet<&str> = allowed.iter().copied().collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let key = value.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        if !allowed.contains(key.as_str()) {
            return Err(invalid(format!("{} 包含不支持的值 {}", field, value)));
        }
        if seen.insert(key.clone()) {
            out.push(key);
        }
    }
    Ok(out)
}

fn normalize_map(
    values: &HashMap<String, String>,
    allowed_keys: &[&str],
    field: &str,
) -> Result<HashMap<String, String>, String> {
    if values.len() > MAX_HINTS {
        return Err(invalid(format!("{} 条目过多", field)));
    }
    let allowed: HashSet<&str> = allowed_keys.iter().copied().collect();
    let mut out = HashMap::new();
    for (raw_key, raw_value) in values {
        let key = raw_key.trim().to_lowercase();
        if !allowed.contains(key.as_str()) {
            return Err(invalid(format!("{} 包含不支持的键 {}", field, raw_key)));
        }
        if out.contains_key(&key) {
            return Err(invalid(format!("{} 规范化后包含重复键 {}", field, key)));
        }
        let value = clean_text(raw_value, field)?;
        out.insert(key, value);
    }
    Ok(out)
}

pub fn normalize_settings(mut settings: AppSettings) -> Result<AppSettings, String> {
    if !(1..=MAX_LOG_RETENTION_DAYS).contains(&settings.log_retention_days) {
        return Err(invalid(format!(
            "日志保留天数必须在 1-{} 天之间",
            MAX_LOG_RETENTION_DAYS
        )));
    }

    settings.theme = settings.theme.trim().to_lowercase();
    if settings.theme != "dark" && settings.theme != "light" {
        return Err(invalid("界面主题仅支持 dark 或 light"));
    }

    if settings.extra_protected_process_names.len() > MAX_PROTECTED_NAMES {
        return Err(invalid("额外受保护进程名条目过多"));
    }
    let mut protected = Vec::new();
    let mut seen = HashSet::new();
    for raw in &settings.extra_protected_process_names {
        let value = clean_text(raw, "受保护进程名")?;
        if value.contains('\\') || value.contains('/') {
            return Err(invalid("受保护进程名只能填写进程名，不能填写路径"));
        }
        let identity = value.to_lowercase();
        if seen.insert(identity) {
            protected.push(value);
        }
    }
    settings.extra_protected_process_names = protected;

    settings.disabled_runtime_kinds = normalize_kind_list(
        &settings.disabled_runtime_kinds,
        RUNTIME_KINDS,
        "禁用运行时",
    )?;
    settings.managed_service_kinds = normalize_kind_list(
        &settings.managed_service_kinds,
        MANAGED_SERVICE_KINDS,
        "基础服务类型",
    )?;
    settings.detection_path_hints = normalize_map(
        &settings.detection_path_hints,
        RUNTIME_KINDS,
        "环境路径提示",
    )?;
    settings.managed_service_name_hints = normalize_map(
        &settings.managed_service_name_hints,
        MANAGED_SERVICE_KINDS,
        "服务名映射",
    )?;

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_lists_and_keys() {
        let mut settings = AppSettings {
            extra_protected_process_names: vec![" node.exe ".to_string(), "NODE.EXE".to_string()],
            disabled_runtime_kinds: vec![" JAVA ".to_string(), "java".to_string()],
            managed_service_kinds: vec![" Redis ".to_string()],
            ..Default::default()
        };
        settings
            .detection_path_hints
            .insert(" NODE ".to_string(), " C:\\Tools\\node.exe ".to_string());

        let normalized = normalize_settings(settings).expect("valid settings");
        assert_eq!(normalized.extra_protected_process_names, vec!["node.exe"]);
        assert_eq!(normalized.disabled_runtime_kinds, vec!["java"]);
        assert_eq!(normalized.managed_service_kinds, vec!["redis"]);
        assert_eq!(
            normalized
                .detection_path_hints
                .get("node")
                .map(String::as_str),
            Some("C:\\Tools\\node.exe")
        );
    }

    #[test]
    fn rejects_invalid_ranges_and_unknown_kinds() {
        let settings = AppSettings {
            log_retention_days: 0,
            ..Default::default()
        };
        assert!(normalize_settings(settings).is_err());

        let settings = AppSettings {
            disabled_runtime_kinds: vec!["go".to_string()],
            ..Default::default()
        };
        assert!(normalize_settings(settings).is_err());
    }

    #[test]
    fn rejects_process_paths_and_control_characters() {
        let settings = AppSettings {
            extra_protected_process_names: vec!["C:\\Windows\\cmd.exe".to_string()],
            ..Default::default()
        };
        assert!(normalize_settings(settings).is_err());

        let mut settings = AppSettings::default();
        settings
            .managed_service_name_hints
            .insert("mysql".to_string(), "MySQL80\nInjected".to_string());
        assert!(normalize_settings(settings).is_err());
    }

    #[test]
    fn rejects_case_insensitive_duplicate_map_keys() {
        let mut settings = AppSettings::default();
        settings
            .detection_path_hints
            .insert("NODE".to_string(), "C:\\node-a.exe".to_string());
        settings
            .detection_path_hints
            .insert("node".to_string(), "C:\\node-b.exe".to_string());
        assert!(normalize_settings(settings).is_err());
    }
}
