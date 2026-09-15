use crate::app_state::AppState;
use crate::domain::{LaunchProfile, LaunchSessionInfo, LaunchSessionState, ProcessRole};
use crate::security_guard;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tauri::State;
use uuid::Uuid;

fn launch_start_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[tauri::command]
pub fn save_launch_profile_safe(
    state: State<'_, AppState>,
    project_id: String,
    process_role: String,
    working_directory: String,
    command: String,
    source_candidate_id: Option<String>,
) -> Result<String, String> {
    let command = command.trim().to_string();
    security_guard::validate_command_policy(&command)?;

    let working_directory = std::fs::canonicalize(Path::new(&working_directory))
        .map_err(|_| "WORKDIR_INVALID:工作目录不存在".to_string())?
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();

    if !state
        .config
        .list_projects()
        .iter()
        .any(|project| project.project_id == project_id)
    {
        return Err("PROJECT_NOT_FOUND:请先添加项目".to_string());
    }

    let role = match process_role.to_lowercase().as_str() {
        "frontend" => ProcessRole::Frontend,
        "backend" => ProcessRole::Backend,
        _ => return Err("PROFILE_INVALID:无效角色".to_string()),
    };

    let profiles = state.config.list_profiles_for_project(&project_id);
    let existing = profiles.into_iter().find(|profile| {
        let same_candidate = source_candidate_id
            .as_ref()
            .zip(profile.source_candidate_id.as_ref())
            .is_some_and(|(left, right)| left == right);
        let same_slot = profile.process_role == role
            && profile
                .working_directory
                .eq_ignore_ascii_case(&working_directory);
        same_candidate || same_slot
    });

    if let Some(mut existing) = existing {
        let is_active = state.command_runner.list_sessions().iter().any(|session| {
            session.profile_id == existing.profile_id
                && matches!(
                    session.state,
                    LaunchSessionState::Starting
                        | LaunchSessionState::Running
                        | LaunchSessionState::Stopping
                )
        });
        if is_active {
            return Err("PROFILE_RUNNING:运行中的启动配置不能修改".to_string());
        }

        existing.process_role = role;
        existing.working_directory = working_directory;
        existing.command = command;
        existing.source_candidate_id = source_candidate_id;
        existing.user_modified = true;
        let profile_id = existing.profile_id.clone();
        state.config.upsert_launch_profile(existing)?;
        return Ok(profile_id);
    }

    let profile_id = Uuid::new_v4().to_string();
    state.config.upsert_launch_profile(LaunchProfile {
        profile_id: profile_id.clone(),
        project_id,
        process_role: role,
        working_directory,
        command,
        source_candidate_id,
        user_modified: true,
        port_hint: None,
    })?;
    Ok(profile_id)
}

#[tauri::command]
pub fn start_launch_profile_safe(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    confirmation_token: String,
) -> Result<LaunchSessionInfo, String> {
    let _start_guard = launch_start_lock()
        .lock()
        .map_err(|_| "LAUNCH_START_FAILED:启动锁失败".to_string())?;

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
