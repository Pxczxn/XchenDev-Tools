use crate::app_state::AppState;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn config_transaction_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn lock_config_write() -> Result<MutexGuard<'static, ()>, String> {
    config_transaction_lock()
        .lock()
        .map_err(|_| "CONFIG_TRANSACTION_LOCK_FAILED:配置事务锁失败".to_string())
}

/// ConfigStore 的部分写方法会先修改内存再持久化。
/// 对用户配置类写操作统一执行“快照 → 写入 → 必要时回滚”，并由全局
/// 配置事务锁串行化，避免并发写时一个失败事务覆盖另一个成功事务。
pub(crate) fn with_config_rollback<T>(
    state: &AppState,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _transaction_guard = lock_config_write()?;
    let snapshot = state.config.export_json()?;

    match operation() {
        Ok(value) => Ok(value),
        Err(original_error) => {
            let rollback_result = state.config.import_json(&snapshot);
            if let Err(rollback_error) = rollback_result {
                // import_json 在持久化前已经把内存替换为快照；即使磁盘仍不可写，
                // 当前进程内也不会继续保留失败操作造成的半状态。
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
