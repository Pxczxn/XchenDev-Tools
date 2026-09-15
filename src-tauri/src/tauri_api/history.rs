use crate::app_state::AppState;
use crate::domain::{AuditEvent, RecentError};
use crate::history_retention::{retained_audit_events, retained_recent_errors};
use chrono::Utc;
use tauri::State;

#[tauri::command]
pub fn list_audit_events_safe(state: State<'_, AppState>, limit: usize) -> Vec<AuditEvent> {
    let settings = state.config.get_settings();
    let events = state.config.list_audit_events(usize::MAX);
    retained_audit_events(events, settings.log_retention_days, Utc::now(), limit)
}

#[tauri::command]
pub fn list_recent_errors_safe(state: State<'_, AppState>, limit: usize) -> Vec<RecentError> {
    let settings = state.config.get_settings();
    let errors = state.config.list_recent_errors(usize::MAX);
    retained_recent_errors(errors, settings.log_retention_days, Utc::now(), limit)
}
