use crate::app_state::{AppState, LAUNCH_CONFIRMATION_TTL_SECS};
use crate::domain::{LaunchConfirmation, ProcessRole};
use crate::security_guard;
use chrono::{Duration, Utc};
use tauri::State;

use super::launch_profiles::validated_workdir_for_project;

#[tauri::command]
pub fn issue_launch_confirmation_safe(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<LaunchConfirmation, String> {
    let profile = state
        .config
        .get_profile(&profile_id)
        .ok_or_else(|| "PROFILE_NOT_FOUND:配置不存在".to_string())?;

    // Confirm exactly the command and directory that would be executed. Imported/manual
    // config can contain aliases or stale paths, so normalize at the confirmation boundary
    // instead of binding a token to raw stored strings.
    let command = profile.command.trim();
    security_guard::validate_command_policy(command)?;
    let canonical_workdir = validated_workdir_for_project(
        &state,
        &profile.project_id,
        &profile.working_directory,
    )?;

    let role = match profile.process_role {
        ProcessRole::Frontend => "frontend",
        ProcessRole::Backend => "backend",
    };
    let (token, summary) = state.issue_confirmation(
        &profile.profile_id,
        command,
        &canonical_workdir,
        role,
    )?;
    Ok(LaunchConfirmation {
        confirmation_token: token,
        profile_id: profile.profile_id,
        binding_summary: summary,
        expires_at: (Utc::now() + Duration::seconds(LAUNCH_CONFIRMATION_TTL_SECS)).to_rfc3339(),
    })
}
