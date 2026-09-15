use crate::config_store::AppConfig;
use crate::domain::{AuditEvent, RecentError};
use chrono::{DateTime, Duration, Utc};

pub const MAX_AUDIT_EVENTS: usize = 200;
pub const MAX_RECENT_ERRORS: usize = 50;

fn timestamp_is_retained(timestamp: &str, cutoff: &DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.with_timezone(&Utc) >= cutoff.clone())
        // Imported legacy records with malformed timestamps are kept rather than deleted blindly.
        .unwrap_or(true)
}

fn trim_oldest<T>(items: &mut Vec<T>, max_len: usize) {
    if items.len() > max_len {
        let drain = items.len() - max_len;
        items.drain(0..drain);
    }
}

pub fn prune_config_history(config: &mut AppConfig, now: DateTime<Utc>) {
    let retention_days = config.settings.log_retention_days.max(1) as i64;
    let cutoff = now - Duration::days(retention_days);

    config
        .audit_events
        .retain(|event| timestamp_is_retained(&event.timestamp, &cutoff));
    config
        .recent_errors
        .retain(|error| timestamp_is_retained(&error.timestamp, &cutoff));

    trim_oldest(&mut config.audit_events, MAX_AUDIT_EVENTS);
    trim_oldest(&mut config.recent_errors, MAX_RECENT_ERRORS);
}

pub fn retained_audit_events(
    events: Vec<AuditEvent>,
    retention_days: u32,
    now: DateTime<Utc>,
    limit: usize,
) -> Vec<AuditEvent> {
    let cutoff = now - Duration::days(retention_days.max(1) as i64);
    let mut retained: Vec<_> = events
        .into_iter()
        .filter(|event| timestamp_is_retained(&event.timestamp, &cutoff))
        .collect();
    trim_oldest(&mut retained, limit.min(MAX_AUDIT_EVENTS));
    retained
}

pub fn retained_recent_errors(
    errors: Vec<RecentError>,
    retention_days: u32,
    now: DateTime<Utc>,
    limit: usize,
) -> Vec<RecentError> {
    let cutoff = now - Duration::days(retention_days.max(1) as i64);
    let mut retained: Vec<_> = errors
        .into_iter()
        .filter(|error| timestamp_is_retained(&error.timestamp, &cutoff))
        .collect();
    trim_oldest(&mut retained, limit.min(MAX_RECENT_ERRORS));
    retained
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::OperationStatus;

    fn audit(timestamp: &str) -> AuditEvent {
        AuditEvent {
            timestamp: timestamp.to_string(),
            action: "TEST".to_string(),
            target: "target".to_string(),
            result: OperationStatus::Succeeded,
            reason_code: None,
            message: None,
        }
    }

    fn error(timestamp: &str) -> RecentError {
        RecentError {
            timestamp: timestamp.to_string(),
            code: "TEST".to_string(),
            message: "message".to_string(),
            source: "test".to_string(),
        }
    }

    #[test]
    fn prune_removes_records_older_than_retention_window() {
        let now = DateTime::parse_from_rfc3339("2026-09-15T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut config = AppConfig::default();
        config.settings.log_retention_days = 7;
        config.audit_events = vec![audit("2026-09-01T00:00:00Z"), audit("2026-09-14T00:00:00Z")];
        config.recent_errors = vec![error("2026-08-01T00:00:00Z"), error("2026-09-15T07:00:00Z")];

        prune_config_history(&mut config, now);
        assert_eq!(config.audit_events.len(), 1);
        assert_eq!(config.recent_errors.len(), 1);
    }

    #[test]
    fn malformed_legacy_timestamp_is_not_deleted_blindly() {
        let now = Utc::now();
        let retained = retained_audit_events(vec![audit("legacy-unknown")], 7, now, 20);
        assert_eq!(retained.len(), 1);
    }

    #[test]
    fn read_filter_applies_requested_limit_after_time_filter() {
        let now = DateTime::parse_from_rfc3339("2026-09-15T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let retained = retained_audit_events(
            vec![
                audit("2026-09-12T00:00:00Z"),
                audit("2026-09-13T00:00:00Z"),
                audit("2026-09-14T00:00:00Z"),
            ],
            7,
            now,
            2,
        );
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[0].timestamp, "2026-09-13T00:00:00Z");
        assert_eq!(retained[1].timestamp, "2026-09-14T00:00:00Z");
    }
}
