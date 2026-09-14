use crate::app_state::{AppState, LAUNCH_CONFIRMATION_TTL_SECS};
use crate::domain::{LaunchConfirmation, ProcessRole};
use chrono::{Duration, Utc};
use tauri::State;

#[tauri::command]
pub fn issue_launch_confirmation_safe(
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
        expires_at: (Utc::now() + Duration::seconds(LAUNCH_CONFIRMATION_TTL_SECS)).to_rfc3339(),
    })
}
