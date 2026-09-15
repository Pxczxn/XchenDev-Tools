use crate::app_state::AppState;
use crate::domain::{
    LaunchProfile, LaunchSessionInfo, LaunchSessionState, OperationResult, ProcessRole,
};
use crate::security_guard;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::State;
use uuid::Uuid;

use super::config_transaction::with_config_rollback;

pub(crate) fn launch_lifecycle_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_launch_lifecycle() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    launch_lifecycle_lock()
        .lock()
        .map_err(|_| "LAUNCH_LIFECYCLE_LOCK_FAILED:启动关系锁失败".to_string())
}

fn canonical_directory(path: &str, error_code: &str) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(Path::new(path))
        .map_err(|_| format!("{}:目录不存在", error_code))?;
    if !canonical.is_dir() {
        return Err(format!("{}:路径不是目录", error_code));
    }
    Ok(canonical)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

fn validated_workdir_for_project(
    state: &AppState,
    project_id: &str,
    working_directory: &str,
) -> Result<String, String> {
    let project = state
        .config
        .list_projects()
        .into_iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| "PROJECT_NOT_FOUND:请先添加项目".to_string())?;

    let project_root = canonical_directory(&project.root_path, "PROJECT_ROOT_INVALID")?;
    let working_directory_path = canonical_directory(working_directory, "WORKDIR_INVALID")?;
    if !working_directory_path.starts_with(&project_root) {
        return Err("WORKDIR_OUTSIDE_PROJECT:工作目录必须位于当前项目根目录内".to_string());
    }
    Ok(display_path(&working_directory_path))
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
    let _lifecycle_guard = lock_launch_lifecycle()?;

    let command = command.trim().to_string();
    security_guard::validate_command_policy(&command)?;
    let working_directory =
        validated_workdir_for_project(&state, &project_id, &working_directory)?;

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
        with_config_rollback(&state, || state.config.upsert_launch_profile(existing))?;
        return Ok(profile_id);
    }

    let profile_id = Uuid::new_v4().to_string();
    let profile = LaunchProfile {
        profile_id: profile_id.clone(),
        project_id,
        process_role: role,
        working_directory,
        command,
        source_candidate_id,
        user_modified: true,
        port_hint: None,
    };
    with_config_rollback(&state, || state.config.upsert_launch_profile(profile))?;
    Ok(profile_id)
}

#[tauri::command]
pub fn remove_launch_profile_safe(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<OperationResult, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;

    let is_active = state.command_runner.list_sessions().iter().any(|session| {
        session.profile_id == profile_id
            && matches!(
                session.state,
                LaunchSessionState::Starting
                    | LaunchSessionState::Running
                    | LaunchSessionState::Stopping
            )
    });
    if is_active {
        return Err("PROFILE_RUNNING:请先停止该启动配置".to_string());
    }

    let removed = with_config_rollback(&state, || state.config.remove_launch_profile(&profile_id))?;
    if removed {
        Ok(OperationResult::succeeded("启动配置已移除"))
    } else {
        Ok(OperationResult::succeeded("启动配置不存在或已移除"))
    }
}

#[tauri::command]
pub fn start_launch_profile_safe(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    confirmation_token: String,
) -> Result<LaunchSessionInfo, String> {
    let _lifecycle_guard = lock_launch_lifecycle()?;

    let profile = state
        .config
        .get_profile(&profile_id)
        .ok_or_else(|| "PROFILE_NOT_FOUND:配置不存在".to_string())?;

    // Imported or manually edited config is untrusted at the execution boundary.
    security_guard::validate_command_policy(profile.command.trim())?;
    let canonical_workdir = validated_workdir_for_project(
        &state,
        &profile.project_id,
        &profile.working_directory,
    )?;

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
        &canonical_workdir,
        profile.command.trim(),
    )
}
