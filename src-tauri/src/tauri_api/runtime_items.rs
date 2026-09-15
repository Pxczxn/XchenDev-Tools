use crate::app_state::AppState;
use crate::domain::{
    LaunchSessionInfo, LaunchSessionState, ProcessRole, RuntimeItem, RuntimeItemState, RuntimeMode,
    ServiceKind, DTO_VERSION,
};
use crate::service_manager;
use std::collections::HashSet;
use tauri::State;

#[tauri::command]
pub fn list_runtime_items_safe(state: State<'_, AppState>) -> Vec<RuntimeItem> {
    let sessions = state.command_runner.list_sessions();
    build_runtime_items(&state, sessions)
}

fn build_runtime_items(state: &AppState, sessions: Vec<LaunchSessionInfo>) -> Vec<RuntimeItem> {
    let active_sessions: Vec<LaunchSessionInfo> = sessions
        .into_iter()
        .filter(|session| is_active_session(&session.state))
        .collect();
    let active_profiles: HashSet<String> = active_sessions
        .iter()
        .map(|session| session.profile_id.clone())
        .collect();

    let mut items = Vec::new();
    for session in active_sessions {
        let profile = state.config.get_profile(&session.profile_id);
        let name = profile
            .map(|profile| format!("{} ({})", profile.command, role_label(&profile.process_role)))
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
        for service in services {
            items.push(RuntimeItem {
                dto_version: DTO_VERSION,
                id: format!("service:{}", service.service_name),
                name: format!("{} [{}]", service.display_name, service_kind_label(&service.kind)),
                runtime_mode: RuntimeMode::WindowsService,
                state: service_manager::service_status_to_runtime_state(&service.status),
            });
        }
    }

    items
}

fn is_active_session(state: &LaunchSessionState) -> bool {
    matches!(
        state,
        LaunchSessionState::Starting | LaunchSessionState::Running | LaunchSessionState::Stopping
    )
}

fn role_label(role: &ProcessRole) -> &'static str {
    match role {
        ProcessRole::Frontend => "前端",
        ProcessRole::Backend => "后端",
    }
}

fn service_kind_label(kind: &ServiceKind) -> &'static str {
    match kind {
        ServiceKind::Mysql => "MySQL",
        ServiceKind::Redis => "Redis",
        ServiceKind::Unknown => "服务",
    }
}

fn map_session_state(session: &LaunchSessionInfo) -> RuntimeItemState {
    match session.state {
        LaunchSessionState::Starting => RuntimeItemState::Starting,
        LaunchSessionState::Running => RuntimeItemState::Running,
        LaunchSessionState::Stopping => RuntimeItemState::Stopping,
        LaunchSessionState::Stopped => RuntimeItemState::Stopped,
        LaunchSessionState::Failed => RuntimeItemState::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_live_states_are_active() {
        assert!(is_active_session(&LaunchSessionState::Starting));
        assert!(is_active_session(&LaunchSessionState::Running));
        assert!(is_active_session(&LaunchSessionState::Stopping));
        assert!(!is_active_session(&LaunchSessionState::Stopped));
        assert!(!is_active_session(&LaunchSessionState::Failed));
    }
}
