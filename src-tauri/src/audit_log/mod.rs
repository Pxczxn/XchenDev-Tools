use crate::config_store::ConfigStore;
use crate::domain::{AuditEvent, OperationStatus, RecentError};
use chrono::Utc;

pub fn record_action(
    store: &ConfigStore,
    action: &str,
    target: &str,
    result: OperationStatus,
    reason_code: Option<String>,
    message: Option<String>,
) {
    let event = AuditEvent {
        timestamp: Utc::now().to_rfc3339(),
        action: action.to_string(),
        target: target.to_string(),
        result,
        reason_code,
        message,
    };
    store.append_audit_event(event);
}

pub fn record_error(store: &ConfigStore, code: &str, message: &str, source: &str) {
    let err = RecentError {
        timestamp: Utc::now().to_rfc3339(),
        code: code.to_string(),
        message: message.to_string(),
        source: source.to_string(),
    };
    store.push_recent_error(err);
}
