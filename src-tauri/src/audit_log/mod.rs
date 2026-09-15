use crate::config_store::ConfigStore;
use crate::config_transaction::lock_config_write;
use crate::domain::{AuditEvent, OperationStatus, RecentError};
use crate::history_retention::prune_config_history;
use chrono::Utc;

fn prune_before_history_write(store: &ConfigStore) {
    if let Ok(mut config) = store.config.lock() {
        prune_config_history(&mut config, Utc::now());
    }
}

pub fn record_action(
    store: &ConfigStore,
    action: &str,
    target: &str,
    result: OperationStatus,
    reason_code: Option<String>,
    message: Option<String>,
) {
    let Ok(_write_guard) = lock_config_write() else {
        return;
    };
    prune_before_history_write(store);
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
    let Ok(_write_guard) = lock_config_write() else {
        return;
    };
    prune_before_history_write(store);
    let err = RecentError {
        timestamp: Utc::now().to_rfc3339(),
        code: code.to_string(),
        message: message.to_string(),
        source: source.to_string(),
    };
    store.push_recent_error(err);
}
