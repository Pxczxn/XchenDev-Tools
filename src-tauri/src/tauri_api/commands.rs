use crate::app_state::AppState;
use crate::config_store::project_id_from_path;
use crate::domain::{
    DirectoryProcessMatch, DTO_VERSION, HealthCheckResponse, LaunchConfirmation, LaunchProfile,
    LaunchSessionInfo, LaunchSessionState, OperationResult, ProcessRole, ProjectScanResult,
    RuntimeItem, RuntimeItemState, RuntimeMode,
};
use crate::audit_log;
use crate::domain::{
    AppSettings, AuditEvent, RecentError, ServiceKind, WindowsServiceInfo,
};
use crate::environment_detector;
use crate::port_manager;
use crate::process_manager;
use crate::project_scanner;
use crate::security_guard;
use crate::service_manager;
use chrono::Utc;
use std::collections::HashSet;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub fn health_check() -> Result<HealthCheckResponse, crate::error::AppError> {
    if std::env::var("XCHEN_IPC_SIMULATE_FAILURE").as_deref() == Ok("1") {
        return Err(crate::error::AppError::IpcNotReady);
    }
    Ok(HealthCheckResponse {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        ipc_status: "ONLINE".to_string(),
    })
}

#[tauri::command]
pub fn list_runtime_items(state: State<'_, AppState>) -> Vec<RuntimeItem> {
    let sessions = state.command_runner.list_sessions();
    let active_profiles: HashSet<String> = sessions
        .iter()
        .map(|s| s.profile_id.clone())
        .collect();
    let mut items = Vec::new();
    for session in sessions {
        let profile = state.config.get_profile(&session.profile_id);
        let name = profile
            .map(|p| format!("{} ({})", p.command, role_label(&p.process_role)))
            .unwrap_or_else(|| session.profile_id.clone());
        items.push(RuntimeItem {
            dto_version: DTO_VERSION,
            id: session.launch_session_id.clone(),
            name,
            runtime_mode: RuntimeMode::CommandProcess,
            state: map_session_state(&session),
        });
    }
    for profile in state.config.all_launch_profiles() {
        if active_profiles.contains(&profile.profile_id) {
            continue;
        }
        items.push(RuntimeItem {
            dto_version: DTO_VERSION,
            id: profile.profile_id.clone(),
            name: format!("{} ({})", profile.command, role_label(&profile.process_role)),
            runtime_mode: RuntimeMode::CommandProcess,
            state: RuntimeItemState::Configured,
        });
    }
    let settings = state.config.get_settings();
    if let Ok(services) = service_manager::list_managed_services(
        &settings.managed_service_kinds,
        &settings.managed_service_name_hints,
    ) {
        for svc in services {
            items.push(RuntimeItem {
                dto_version: DTO_VERSION,
                id: format!("service:{}", svc.service_name),
                name: format!("{} [{}]", svc.display_name, service_kind_label(&svc.kind)),
                runtime_mode: RuntimeMode::WindowsService,
                state: service_manager::service_status_to_runtime_state(&svc.status),
            });
        }
    }
    items
}

fn service_kind_label(kind: &ServiceKind) -> &'static str {
    match kind {
        ServiceKind::Mysql => "MySQL",
        ServiceKind::Redis => "Redis",
        ServiceKind::Unknown => "服务",
    }
}

#[tauri::command]
pub fn list_launch_sessions(state: State<'_, AppState>) -> Vec<LaunchSessionInfo> {
    state.command_runner.list_sessions()
}

fn role_label(role: &ProcessRole) -> &'static str {
    match role {
        ProcessRole::Frontend => "前端",
        ProcessRole::Backend => "后端",
    }
}

fn map_session_state(session: &LaunchSessionInfo) -> RuntimeItemState {
    match session.state {
        LaunchSessionState::Starting => RuntimeItemState::Starting,
        LaunchSessionState::Running => RuntimeItemState::Running,
        LaunchSessionState::Stopping => RuntimeItemState::Stopping,
        LaunchSessionState::Stopped => {
            if session.exit_code.unwrap_or(0) != 0 {
                RuntimeItemState::Failed
            } else {
                RuntimeItemState::Stopped
            }
        }
        LaunchSessionState::Failed => RuntimeItemState::Failed,
    }
}

#[tauri::command]
pub fn list_environment_candidates(
    state: State<'_, AppState>,
    runtime_kinds: Vec<String>,
    force_refresh: Option<bool>,
) -> Result<Vec<crate::domain::EnvironmentCandidate>, String> {
    let force_refresh = force_refresh.unwrap_or(false);
    if let Some(cached) = state.cached_environment_candidates(&runtime_kinds, force_refresh)? {
        return Ok(cached);
    }
    let candidates = environment_detector::list_candidates(&state.config, &runtime_kinds)?;
    state.store_environment_candidates(&runtime_kinds, candidates.clone())?;
    Ok(candidates)
}

#[tauri::command]
pub fn save_manual_override(
    state: State<'_, AppState>,
    runtime_kind: String,
    executable_path: String,
) -> Result<crate::domain::EnvironmentCandidate, String> {
    let result =
        environment_detector::validate_and_save_override(&state.config, &runtime_kind, &executable_path)?;
    state.invalidate_env_detection_cache();
    Ok(result)
}

#[tauri::command]
pub fn inspect_port(
    state: State<'_, AppState>,
    protocol: String,
    port: u16,
) -> Result<Vec<crate::domain::PortOccupancy>, String> {
    let extra = state.config.extra_protected_names();
    let mut rows = port_manager::inspect_port(&protocol, port)?;
    for row in &mut rows {
        row.protection = process_manager::protection_with_extra(&row.process, &extra);
    }
    Ok(rows)
}

#[tauri::command]
pub fn terminate_process(
    state: State<'_, AppState>,
    pid: u32,
    mode: String,
    confirmation_token: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<OperationResult, String> {
    if confirmation_token.is_empty() {
        return Ok(OperationResult::rejected("TERMINATE_DENIED", "需要确认令牌"));
    }
    let extra = state.config.extra_protected_names();
    process_manager::verify_process_snapshot(pid, &expected_name, expected_cwd.as_deref())?;
    let force = mode.eq_ignore_ascii_case("force");
    let target = format!("pid:{}:{}", pid, expected_name);
    match process_manager::terminate_pid_with_extra(pid, force, &extra) {
        Ok(()) => {
            audit_log::record_action(
                &state.config,
                "TERMINATE_PROCESS",
                &target,
                crate::domain::OperationStatus::Succeeded,
                None,
                Some("进程已终止".to_string()),
            );
            Ok(OperationResult::succeeded("进程已终止"))
        }
        Err(e) => {
            let code = e.split(':').next().unwrap_or("TERMINATE_FAILED");
            audit_log::record_action(
                &state.config,
                "TERMINATE_PROCESS",
                &target,
                crate::domain::OperationStatus::Rejected,
                Some(code.to_string()),
                Some(e.clone()),
            );
            audit_log::record_error(&state.config, code, &e, "terminate_process");
            Err(e)
        }
    }
}

#[tauri::command]
pub fn inspect_directory_processes(
    state: State<'_, AppState>,
    root_path: String,
) -> Result<Vec<DirectoryProcessMatch>, String> {
    let extra = state.config.extra_protected_names();
    let root = security_guard::normalize_directory(&root_path)?;
    let summaries = process_manager::list_all_summaries();
    let mut matches = Vec::new();
    for summary in summaries {
        let cwd = match &summary.working_directory {
            Some(c) => c,
            None => continue,
        };
        let level = security_guard::directory_matches_prefix(cwd, &root);
        if level.is_none() {
            continue;
        }
        let ports = port_manager::ports_for_pid(summary.pid);
        let digest = process_manager::digest_for(
            summary.pid,
            &summary.name,
            summary.working_directory.as_deref(),
        );
        let protection = process_manager::protection_with_extra(&summary, &extra);
        matches.push(DirectoryProcessMatch {
            match_level: level.unwrap(),
            pid: summary.pid,
            parent_pid: None,
            name: summary.name,
            working_directory: summary.working_directory,
            command_line: summary.command_line,
            ports,
            protection,
            snapshot_digest: digest,
        });
    }
    Ok(matches)
}

#[tauri::command]
pub fn terminate_directory_process(
    state: State<'_, AppState>,
    pid: u32,
    snapshot_digest: String,
    mode: String,
    confirmation_token: String,
    expected_name: String,
    expected_cwd: Option<String>,
) -> Result<OperationResult, String> {
    if confirmation_token.is_empty() {
        return Ok(OperationResult::rejected("TERMINATE_DENIED", "需要确认令牌"));
    }
    let digest = process_manager::digest_for(
        pid,
        &expected_name,
        expected_cwd.as_deref(),
    );
    if digest != snapshot_digest {
        return Ok(OperationResult::rejected(
            "PROCESS_SNAPSHOT_MISMATCH",
            "进程快照已失效，请刷新",
        ));
    }
    terminate_process(
        state,
        pid,
        mode,
        confirmation_token,
        expected_name,
        expected_cwd,
    )
}

#[tauri::command]
pub fn scan_project_directory(root_path: String) -> Result<ProjectScanResult, String> {
    project_scanner::scan_project_directory(&root_path)
}

#[tauri::command]
pub fn save_launch_profile(
    state: State<'_, AppState>,
    project_id: String,
    process_role: String,
    working_directory: String,
    command: String,
    source_candidate_id: Option<String>,
) -> Result<String, String> {
    security_guard::validate_command_policy(&command)?;
    if !std::path::Path::new(&working_directory).exists() {
        return Err("WORKDIR_INVALID:工作目录不存在".to_string());
    }
    let role = match process_role.to_lowercase().as_str() {
        "frontend" => ProcessRole::Frontend,
        "backend" => ProcessRole::Backend,
        _ => return Err("PROFILE_INVALID:无效角色".to_string()),
    };
    let profile_id = Uuid::new_v4().to_string();
    let profile = LaunchProfile {
        profile_id: profile_id.clone(),
        project_id,
        process_role: role,
        working_directory,
        command,
        source_candidate_id,
        user_modified: false,
        port_hint: None,
    };
    state.config.add_profile(profile)?;
    Ok(profile_id)
}

#[tauri::command]
pub fn list_launch_profiles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<LaunchProfile>, String> {
    Ok(state.config.list_profiles_for_project(&project_id))
}

#[tauri::command]
pub fn issue_launch_confirmation(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<LaunchConfirmation, String> {
    let profile = state
        .config
        .get_profile(&profile_id)
        .ok_or_else(|| "PROFILE_NOT_FOUND:配置不存在".to_string())?;
    let role = match profile.process_role {
        ProcessRole::Frontend => "frontend",
        ProcessRole::Backend => "backend",
    };
    let (token, summary) = state.issue_confirmation(
        &profile.profile_id,
        &profile.command,
        &profile.working_directory,
        role,
    )?;
    Ok(LaunchConfirmation {
        confirmation_token: token,
        profile_id: profile.profile_id,
        binding_summary: summary,
        expires_at: Utc::now().to_rfc3339(),
    })
}

#[tauri::command]
pub fn start_launch_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    confirmation_token: String,
) -> Result<crate::domain::LaunchSessionInfo, String> {
    let profile = state
        .config
        .get_profile(&profile_id)
        .ok_or_else(|| "PROFILE_NOT_FOUND:配置不存在".to_string())?;
    let role = match profile.process_role {
        ProcessRole::Frontend => "frontend",
        ProcessRole::Backend => "backend",
    };
    state.consume_confirmation(
        &confirmation_token,
        &profile.profile_id,
        &profile.command,
        &profile.working_directory,
        role,
    )?;
    state.command_runner.start(
        &app,
        &profile.profile_id,
        &profile.working_directory,
        &profile.command,
    )
}

#[tauri::command]
pub fn stop_launch_session(
    state: State<'_, AppState>,
    launch_session_id: String,
) -> Result<OperationResult, String> {
    state
        .command_runner
        .stop(&launch_session_id)
        .map_err(|e| e)?;
    Ok(OperationResult::succeeded("会话已停止"))
}

#[tauri::command]
pub fn project_id_for_path(root_path: String) -> String {
    project_id_from_path(&root_path)
}

#[tauri::command]
pub fn export_app_config(state: State<'_, AppState>) -> Result<String, String> {
    state.config.export_json()
}

#[tauri::command]
pub fn import_app_config(state: State<'_, AppState>, content: String) -> Result<OperationResult, String> {
    state.config.import_json(&content)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[tauri::command]
pub fn import_app_config_from_path(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<OperationResult, String> {
    let data = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("PROFILE_INVALID:无法读取文件 {}", e))?;
    state.config.import_json(&data)?;
    Ok(OperationResult::succeeded("配置已导入"))
}

#[tauri::command]
pub fn export_app_config_to_path(
    state: State<'_, AppState>,
    target_path: String,
) -> Result<OperationResult, String> {
    state.config.write_export_to_path(&target_path)?;
    Ok(OperationResult::succeeded("配置已导出"))
}

#[tauri::command]
pub fn get_config_paths(state: State<'_, AppState>) -> (String, String) {
    let file = state.config.config_file_path();
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    (dir, file.to_string_lossy().to_string())
}

#[tauri::command]
pub fn list_managed_services(state: State<'_, AppState>) -> Result<Vec<WindowsServiceInfo>, String> {
    let settings = state.config.get_settings();
    service_manager::list_managed_services(
        &settings.managed_service_kinds,
        &settings.managed_service_name_hints,
    )
}

#[tauri::command]
pub fn control_windows_service(
    state: State<'_, AppState>,
    service_name: String,
    action: String,
    confirmation_token: String,
) -> Result<OperationResult, String> {
    if confirmation_token.is_empty() {
        return Ok(OperationResult::rejected("SERVICE_CONTROL_DENIED", "需要确认令牌"));
    }
    let target = format!("service:{}:{}", service_name, action);
    match service_manager::control_service(&service_name, &action) {
        Ok(()) => {
            audit_log::record_action(
                &state.config,
                "CONTROL_WINDOWS_SERVICE",
                &target,
                crate::domain::OperationStatus::Succeeded,
                None,
                Some("服务操作已提交".to_string()),
            );
            Ok(OperationResult::succeeded("服务操作已提交"))
        }
        Err(e) => {
            let code = e.split(':').next().unwrap_or("SERVICE_CONTROL_FAILED");
            audit_log::record_action(
                &state.config,
                "CONTROL_WINDOWS_SERVICE",
                &target,
                crate::domain::OperationStatus::Failed,
                Some(code.to_string()),
                Some(e.clone()),
            );
            audit_log::record_error(&state.config, code, &e, "control_windows_service");
            Err(e)
        }
    }
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> AppSettings {
    state.config.get_settings()
}

#[tauri::command]
pub fn save_app_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<OperationResult, String> {
    state.config.save_settings(settings)?;
    state.invalidate_env_detection_cache();
    Ok(OperationResult::succeeded("设置已保存"))
}

#[tauri::command]
pub fn list_audit_events(state: State<'_, AppState>, limit: usize) -> Vec<AuditEvent> {
    state.config.list_audit_events(limit)
}

#[tauri::command]
pub fn list_recent_errors(state: State<'_, AppState>, limit: usize) -> Vec<RecentError> {
    state.config.list_recent_errors(limit)
}

#[tauri::command]
pub fn list_default_protected_processes() -> Vec<String> {
    security_guard::default_protected_process_names()
}

pub fn make_runtime_item_placeholder() -> RuntimeItem {
    RuntimeItem {
        dto_version: DTO_VERSION,
        id: "placeholder".to_string(),
        name: "无运行项".to_string(),
        runtime_mode: RuntimeMode::CommandProcess,
        state: RuntimeItemState::Discovered,
    }
}
