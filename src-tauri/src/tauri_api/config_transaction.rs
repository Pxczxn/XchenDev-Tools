use crate::app_state::AppState;

/// ConfigStore 目前的写方法会先修改内存再持久化。
/// 安全 IPC 层统一在调用前保留完整 JSON 快照；如果落盘失败，
/// 再用旧快照恢复内存。即使恢复时磁盘仍不可写，import_json 也会
/// 在持久化前先把内存替换回旧值，因此不会留下“内存成功、磁盘失败”的半状态。
pub(crate) fn with_config_rollback<T>(
    state: &AppState,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let snapshot = state.config.export_json()?;
    match operation() {
        Ok(value) => Ok(value),
        Err(original_error) => {
            let rollback_result = state.config.import_json(&snapshot);
            if let Err(rollback_error) = rollback_result {
                // import_json 在持久化前已经把内存替换为快照；这里保留原始
                // 操作错误，同时附上回滚落盘失败信息，方便审计磁盘故障。
                return Err(format!(
                    "{}; CONFIG_ROLLBACK_PERSIST_FAILED:{}",
                    original_error, rollback_error
                ));
            }
            Err(original_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_store::open_at;
    use tempfile::tempdir;

    #[test]
    fn successful_operation_keeps_new_state() {
        let dir = tempdir().expect("tempdir");
        let mut state = AppState::new();
        state.config = open_at(dir.path().join("config.json"));
        let mut settings = state.config.get_settings();
        settings.log_retention_days = 14;

        with_config_rollback(&state, || state.config.save_settings(settings.clone()))
            .expect("save");
        assert_eq!(state.config.get_settings().log_retention_days, 14);
    }
}
