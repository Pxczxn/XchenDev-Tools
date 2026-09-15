use crate::app_state::AppState;
use crate::config_store::AppConfig;
use crate::config_transaction::with_config_rollback;
use crate::domain::{LaunchSessionState, OperationResult, ProcessRole};
use crate::history_retention::prune_config_history;
use crate::security_guard;
use crate::settings_guard::normalize_settings;
use chrono::Utc;
use std::collections::HashSet;
use tauri::State;

use super::launch_profiles::launch_lifecycle_lock;

fn lock_launch_lifecycle() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    launch_lifecycle_lock()
        .lock()
        .map_err(|_| "LAUNCH_LIFECYCLE_LOCK_FAILED:启动关系锁失败".to_string())
}

fn ensure_no_active_launch_sessions(state: &AppState) -> Result<(), String> {
    let has_active = state.command_runner.list_sessions().iter().any(|session| {
        matches!(
            session.state,
            LaunchSessionState::Starting
                | LaunchSessionState::Running
                | LaunchSessionState::Stopping
        )
    });
    if has_active {
        return Err(
            "CONFIG_IMPORT_BLOCKED_ACTIVE_SESSIONS:请先停止所有项目运行会话再导入配置"
                .to_string(),
        );
    }
    Ok(())
}

fn role_key(role: &ProcessRole) -> &'static str {
    match role {
        ProcessRole::Frontend => "frontend",
        ProcessRole::Backend => "backend",
    }
}

fn logical_workdir_key(path: &str) -> String {
    let normalized = path.trim().replace('/', "\\");
    let without_extended_prefix = if let Some(rest) = normalized.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{}", rest)
    } else if let Some(rest) = normalized.strip_prefix("\\\\?\\") {
        rest.to_string()
    } else {
        normalized
    };
    without_extended_prefix
        .trim_end_matches('\\')
        .to_lowercase()
}

fn validate_import_projects(config: &AppConfig) -> Result<(), String> {
    let mut project_ids = HashSet::new();
    let mut roots = HashSet::new();
    for project in &config.projects {
        if project.project_id.trim().is_empty() {
            return Err("PROFILE_INVALID:项目 ID 不能为空".to_string());
        }
        if !project_ids.insert(project.project_id.clone()) {
            return Err(format!(
                "PROFILE_INVALID:存在重复项目 ID {}",
                project.project_id
            ));
        }

        let root_key = logical_workdir_key(&project.root_path);
        if root_key.is_empty() {
            return Err(format!(
                "PROFILE_INVALID:项目 {} 的根目录不能为空",
                project.project_id
            ));
        }
        if !roots.insert(root_key) {
            return Err(format!(
                "PROFILE_INVALID:存在重复项目根目录 {}",
                project.root_path
            ));
        }
    }
    Ok(())
}

fn validate_import_profiles(config: &AppConfig) -> Result<(), String> {
    let project_ids: HashSet<&str> = config
        .projects
        .iter()
        .map(|project| project.project_id.as_str())
        .collect();
    let mut profile_ids = HashSet::new();
    let mut slots = HashSet::new();
    let mut candidates = HashSet::new();

    for profile in &config.launch_profiles {
        if profile.profile_id.trim().is_empty() {
            return Err("PROFILE_INVALID:启动配置 ID 不能为空".to_string());
        }
        if !profile_ids.insert(profile.profile_id.clone()) {
            return Err(format!(
                "PROFILE_INVALID:存在重复启动配置 ID {}",
                profile.profile_id
            ));
        }
        if !project_ids.contains(profile.project_id.as_str()) {
            return Err(format!(
                "PROFILE_INVALID:启动配置 {} 引用了不存在的项目 {}",
                profile.profile_id, profile.project_id
            ));
        }

        security_guard::validate_command_policy(profile.command.trim())?;

        let workdir_key = logical_workdir_key(&profile.working_directory);
        if workdir_key.is_empty() {
            return Err(format!(
                "PROFILE_INVALID:启动配置 {} 的工作目录不能为空",
                profile.profile_id
            ));
        }

        let slot = format!(
            "{}|{}|{}",
            profile.project_id,
            role_key(&profile.process_role),
            workdir_key
        );
        if !slots.insert(slot) {
            return Err(format!(
                "PROFILE_INVALID:项目 {} 存在重复的 {} 启动配置工作目录 {}",
                profile.project_id,
                role_key(&profile.process_role),
                profile.working_directory
            ));
        }

        if let Some(candidate_id) = profile
            .source_candidate_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let binding = format!("{}|{}", profile.project_id, candidate_id.to_lowercase());
            if !candidates.insert(binding) {
                return Err(format!(
                    "PROFILE_INVALID:项目 {} 的候选 {} 被多个启动配置重复绑定",
                    profile.project_id, candidate_id
                ));
            }
        }
    }

    Ok(())
}

fn normalize_import_content(content: &str) -> Result<String, String> {
    let mut parsed: AppConfig =
        serde_json::from_str(content).map_err(|e| format!("PROFILE_INVALID:{}", e))?;
    parsed.settings = normalize_settings(parsed.settings)?;
    validate_import_projects(&parsed)?;
    validate_import_profiles(&parsed)?;
    prune_config_history(&mut parsed, Utc::now());
    serde_json::to_string_pretty(&parsed).map_err(|e| format!("PROFILE_INVALID:{}", e))
}

fn import_normalized(state: &AppState, content: &str) -> Result<(), String> {
    let normalized = normalize_import_content(content)?;
    with_config_rollback(state, || state.config.import_json(&normalized))?;
    state.invalidate_env_detection_cache();
    Ok(())
}

#[tauri::command]
pub fn import_app_config_safe(
    state: State<'_, AppState>,
    content: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;
    ensure_no_active_launch_sessions(&state)?;
    import_normalized(&state, &content)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[tauri::command]
pub fn import_app_config_from_path_safe(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;
    ensure_no_active_launch_sessions(&state)?;
    let data = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("PROFILE_INVALID:无法读取文件 {}", e))?;
    import_normalized(&state, &data)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LaunchProfile, ProjectInfo};

    fn project() -> ProjectInfo {
        ProjectInfo {
            project_id: "project-a".to_string(),
            name: "demo".to_string(),
            root_path: "C:\\demo".to_string(),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            updated_at: "2026-09-15T00:00:00Z".to_string(),
        }
    }

    fn profile(id: &str, workdir: &str, candidate: Option<&str>) -> LaunchProfile {
        LaunchProfile {
            profile_id: id.to_string(),
            project_id: "project-a".to_string(),
            process_role: ProcessRole::Frontend,
            working_directory: workdir.to_string(),
            command: "npm run dev".to_string(),
            source_candidate_id: candidate.map(str::to_string),
            user_modified: true,
            port_hint: None,
        }
    }

    #[test]
    fn empty_runner_allows_config_import_guard() {
        let state = AppState::new();
        assert!(ensure_no_active_launch_sessions(&state).is_ok());
    }

    #[test]
    fn import_rejects_invalid_settings() {
        let mut config = AppConfig::default();
        config.settings.log_retention_days = 0;
        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("invalid settings must fail");
        assert!(err.contains("SETTINGS_INVALID"));
    }

    #[test]
    fn import_normalizes_settings_before_store_validation() {
        let mut config = AppConfig::default();
        config.settings.theme = " DARK ".to_string();
        config.settings.disabled_runtime_kinds = vec![" JAVA ".to_string(), "java".to_string()];
        let json = serde_json::to_string(&config).expect("serialize");
        let normalized = normalize_import_content(&json).expect("normalize");
        let parsed: AppConfig = serde_json::from_str(&normalized).expect("deserialize");
        assert_eq!(parsed.settings.theme, "dark");
        assert_eq!(parsed.settings.disabled_runtime_kinds, vec!["java"]);
    }

    #[test]
    fn import_rejects_duplicate_project_id() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config.projects.push(ProjectInfo {
            project_id: "project-a".to_string(),
            name: "other".to_string(),
            root_path: "C:\\other".to_string(),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            updated_at: "2026-09-15T00:00:00Z".to_string(),
        });

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("duplicate project id must fail");
        assert!(err.contains("重复项目 ID"));
    }

    #[test]
    fn import_rejects_duplicate_project_root_variants() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config.projects.push(ProjectInfo {
            project_id: "project-b".to_string(),
            name: "same-root".to_string(),
            root_path: "c:/DEMO/".to_string(),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            updated_at: "2026-09-15T00:00:00Z".to_string(),
        });

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("duplicate roots must fail");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复项目根目录"));
    }

    #[test]
    fn import_rejects_empty_project_root() {
        let mut config = AppConfig::default();
        config.projects.push(ProjectInfo {
            project_id: "project-empty".to_string(),
            name: "empty".to_string(),
            root_path: "  ".to_string(),
            created_at: "2026-09-15T00:00:00Z".to_string(),
            updated_at: "2026-09-15T00:00:00Z".to_string(),
        });

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("empty project root must fail");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("根目录不能为空"));
    }

    #[test]
    fn import_rejects_duplicate_profile_id() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web-a", Some("candidate-a")));
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web-b", Some("candidate-b")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("duplicate profile id must fail");
        assert!(err.contains("重复启动配置 ID"));
    }

    #[test]
    fn import_rejects_orphan_profile_project_reference() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        let mut orphan = profile("profile-a", "C:\\demo\\web", Some("candidate-a"));
        orphan.project_id = "project-missing".to_string();
        config.launch_profiles.push(orphan);

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("orphan profile must fail");
        assert!(err.contains("引用了不存在的项目"));
    }

    #[test]
    fn import_rejects_duplicate_logical_launch_slot() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web", Some("candidate-a")));
        config
            .launch_profiles
            .push(profile("profile-b", "c:\\DEMO\\web", Some("candidate-b")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("duplicate slot must fail");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复"));
    }

    #[test]
    fn import_rejects_duplicate_slot_with_separator_variants() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web", Some("candidate-a")));
        config
            .launch_profiles
            .push(profile("profile-b", "C:/demo/web/", Some("candidate-b")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json)
            .expect_err("slash and trailing-separator variants must be the same slot");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复"));
    }

    #[test]
    fn import_rejects_duplicate_slot_with_extended_path_prefix() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web", Some("candidate-a")));
        config
            .launch_profiles
            .push(profile("profile-b", r"\\?\C:\demo\web\", Some("candidate-b")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json)
            .expect_err("extended path prefix must not create a duplicate slot");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复"));
    }

    #[test]
    fn import_rejects_duplicate_slot_with_extended_unc_prefix() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config.launch_profiles.push(profile(
            "profile-a",
            r"\\server\share\demo\web",
            Some("candidate-a"),
        ));
        config.launch_profiles.push(profile(
            "profile-b",
            r"\\?\UNC\server\share\demo\web\",
            Some("candidate-b"),
        ));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json)
            .expect_err("extended UNC prefix must not create a duplicate slot");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复"));
    }

    #[test]
    fn import_rejects_empty_working_directory() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "   ", Some("candidate-a")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("empty workdir must fail");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("工作目录不能为空"));
    }

    #[test]
    fn import_rejects_duplicate_candidate_binding() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        config
            .launch_profiles
            .push(profile("profile-a", "C:\\demo\\web-a", Some("candidate-a")));
        config
            .launch_profiles
            .push(profile("profile-b", "C:\\demo\\web-b", Some("CANDIDATE-A")));

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("duplicate candidate must fail");
        assert!(err.contains("PROFILE_INVALID"));
        assert!(err.contains("重复绑定"));
    }

    #[test]
    fn import_rejects_unsafe_launch_command_before_storage() {
        let mut config = AppConfig::default();
        config.projects.push(project());
        let mut unsafe_profile = profile("profile-a", "C:\\demo\\web", Some("candidate-a"));
        unsafe_profile.command = "cmd /C echo unsafe".to_string();
        config.launch_profiles.push(unsafe_profile);

        let json = serde_json::to_string(&config).expect("serialize");
        let err = normalize_import_content(&json).expect_err("unsafe command must fail");
        assert!(err.contains("COMMAND_POLICY_REJECTED"));
    }
}
