use crate::command_runner::CommandRunnerState;
use crate::config_store::ConfigStore;
use crate::domain::EnvironmentCandidate;
use crate::process_manager;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const ENV_DETECTION_CACHE_TTL: Duration = Duration::from_secs(120);
pub const LAUNCH_CONFIRMATION_TTL_SECS: i64 = 120;
pub const PROCESS_CONFIRMATION_TTL_SECS: i64 = 60;
const MAX_PENDING_LAUNCH_CONFIRMATIONS: usize = 256;
const MAX_PENDING_PROCESS_CONFIRMATIONS: usize = 256;

#[derive(Clone)]
struct EnvironmentDetectionCache {
    fingerprint: u64,
    kinds_key: String,
    candidates: Vec<EnvironmentCandidate>,
    cached_at: Instant,
}

pub struct AppState {
    pub config: ConfigStore,
    pub command_runner: CommandRunnerState,
    confirmations: Mutex<HashMap<String, PendingConfirmation>>,
    process_confirmations: Mutex<HashMap<String, PendingProcessConfirmation>>,
    env_detection_cache: Mutex<Option<EnvironmentDetectionCache>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingConfirmation {
    pub token: String,
    pub profile_id: String,
    pub binding_digest: String,
    pub created_at: DateTime<Utc>,
    pub consumed: bool,
}

#[derive(Clone)]
struct PendingProcessConfirmation {
    binding_digest: String,
    created_at: DateTime<Utc>,
}

fn make_room_for_pending<T, F>(
    map: &mut HashMap<String, T>,
    max_entries: usize,
    created_at_millis: F,
) where
    F: Fn(&T) -> i64,
{
    while !map.is_empty() && map.len() >= max_entries {
        let oldest = map
            .iter()
            .min_by_key(|(_, item)| created_at_millis(item))
            .map(|(token, _)| token.clone());
        let Some(token) = oldest else {
            break;
        };
        map.remove(&token);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ConfigStore::new(),
            command_runner: CommandRunnerState::new(),
            confirmations: Mutex::new(HashMap::new()),
            process_confirmations: Mutex::new(HashMap::new()),
            env_detection_cache: Mutex::new(None),
        }
    }

    pub fn invalidate_env_detection_cache(&self) {
        if let Ok(mut cache) = self.env_detection_cache.lock() {
            *cache = None;
        }
    }

    pub fn cached_environment_candidates(
        &self,
        runtime_kinds: &[String],
        force_refresh: bool,
    ) -> Result<Option<Vec<EnvironmentCandidate>>, String> {
        if force_refresh {
            return Ok(None);
        }
        let fingerprint = self.config.detection_fingerprint();
        let mut kinds = runtime_kinds.to_vec();
        kinds.sort();
        let kinds_key = kinds.join(",");
        let cache = self
            .env_detection_cache
            .lock()
            .map_err(|_| "DETECT_IO_ERROR:缓存锁失败".to_string())?;
        if let Some(entry) = cache.as_ref() {
            if entry.fingerprint == fingerprint
                && entry.kinds_key == kinds_key
                && entry.cached_at.elapsed() <= ENV_DETECTION_CACHE_TTL
            {
                return Ok(Some(entry.candidates.clone()));
            }
        }
        Ok(None)
    }

    pub fn store_environment_candidates(
        &self,
        runtime_kinds: &[String],
        candidates: Vec<EnvironmentCandidate>,
    ) -> Result<(), String> {
        let fingerprint = self.config.detection_fingerprint();
        let mut kinds = runtime_kinds.to_vec();
        kinds.sort();
        let kinds_key = kinds.join(",");
        let mut cache = self
            .env_detection_cache
            .lock()
            .map_err(|_| "DETECT_IO_ERROR:缓存锁失败".to_string())?;
        *cache = Some(EnvironmentDetectionCache {
            fingerprint,
            kinds_key,
            candidates,
            cached_at: Instant::now(),
        });
        Ok(())
    }

    pub fn issue_confirmation(
        &self,
        profile_id: &str,
        command: &str,
        working_directory: &str,
        role: &str,
    ) -> Result<(String, String), String> {
        let binding = format!("{}|{}|{}|{}", profile_id, command, working_directory, role);
        let mut hasher = Sha256::new();
        hasher.update(binding.as_bytes());
        let digest = hex::encode(hasher.finalize());
        let token = Uuid::new_v4().to_string();
        let summary = format!("{} @ {}", command, working_directory);
        let now = Utc::now();
        let pending = PendingConfirmation {
            token: token.clone(),
            profile_id: profile_id.to_string(),
            binding_digest: digest,
            created_at: now,
            consumed: false,
        };
        let mut confirmations = self
            .confirmations
            .lock()
            .map_err(|_| "CONFIRMATION_ISSUE_FAILED:锁失败".to_string())?;
        confirmations.retain(|_, item| {
            let age = Utc::now()
                .signed_duration_since(item.created_at)
                .num_seconds();
            !item.consumed && (0..=LAUNCH_CONFIRMATION_TTL_SECS).contains(&age)
        });
        make_room_for_pending(
            &mut confirmations,
            MAX_PENDING_LAUNCH_CONFIRMATIONS,
            |item| item.created_at.timestamp_millis(),
        );
        confirmations.insert(token.clone(), pending);
        Ok((token, summary))
    }

    pub fn consume_confirmation(
        &self,
        token: &str,
        profile_id: &str,
        command: &str,
        working_directory: &str,
        role: &str,
    ) -> Result<(), String> {
        let binding = format!("{}|{}|{}|{}", profile_id, command, working_directory, role);
        let mut hasher = Sha256::new();
        hasher.update(binding.as_bytes());
        let digest = hex::encode(hasher.finalize());

        let mut map = self
            .confirmations
            .lock()
            .map_err(|_| "LAUNCH_CONFIRMATION_REQUIRED:确认无效".to_string())?;
        let pending = map
            .get(token)
            .cloned()
            .ok_or_else(|| "LAUNCH_CONFIRMATION_REQUIRED:确认令牌不存在".to_string())?;
        let age_seconds = Utc::now()
            .signed_duration_since(pending.created_at)
            .num_seconds();
        if pending.consumed
            || !(0..=LAUNCH_CONFIRMATION_TTL_SECS).contains(&age_seconds)
            || pending.profile_id != profile_id
            || pending.binding_digest != digest
        {
            map.remove(token);
            return Err("LAUNCH_CONFIRMATION_REQUIRED:确认令牌无效或已过期".to_string());
        }
        map.remove(token);
        Ok(())
    }

    pub fn issue_process_confirmation(
        &self,
        pid: u32,
        expected_name: &str,
        expected_cwd: Option<&str>,
        mode: &str,
    ) -> Result<(String, String), String> {
        let summary = process_manager::find_process_summary(pid)
            .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
        if !summary.name.eq_ignore_ascii_case(expected_name) {
            return Err("PROCESS_SNAPSHOT_MISMATCH:进程名不匹配".to_string());
        }
        if let Some(expected) = expected_cwd {
            match summary.working_directory.as_deref() {
                Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
                _ => return Err("PROCESS_SNAPSHOT_MISMATCH:工作目录不匹配".to_string()),
            }
        }
        let start_time = process_manager::process_start_time_secs(pid)
            .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
        let binding_digest = process_confirmation_digest(
            pid,
            start_time,
            &summary.name,
            summary.working_directory.as_deref(),
            mode,
        );
        let token = Uuid::new_v4().to_string();
        let pending = PendingProcessConfirmation {
            binding_digest,
            created_at: Utc::now(),
        };
        let mut map = self
            .process_confirmations
            .lock()
            .map_err(|_| "PROCESS_CONFIRMATION_ISSUE_FAILED:确认锁失败".to_string())?;
        map.retain(|_, item| {
            let age = Utc::now()
                .signed_duration_since(item.created_at)
                .num_seconds();
            (0..=PROCESS_CONFIRMATION_TTL_SECS).contains(&age)
        });
        make_room_for_pending(&mut map, MAX_PENDING_PROCESS_CONFIRMATIONS, |item| {
            item.created_at.timestamp_millis()
        });
        map.insert(token.clone(), pending);
        let action = if mode.eq_ignore_ascii_case("force") {
            "强制终止"
        } else {
            "终止"
        };
        Ok((token, format!("{} PID {} ({})", action, pid, summary.name)))
    }

    pub fn consume_process_confirmation(
        &self,
        token: &str,
        pid: u32,
        expected_name: &str,
        expected_cwd: Option<&str>,
        mode: &str,
    ) -> Result<(), String> {
        let pending = {
            let mut map = self
                .process_confirmations
                .lock()
                .map_err(|_| "PROCESS_CONFIRMATION_REQUIRED:确认无效".to_string())?;
            let pending = map
                .remove(token)
                .ok_or_else(|| "PROCESS_CONFIRMATION_REQUIRED:确认令牌不存在".to_string())?;
            let age = Utc::now()
                .signed_duration_since(pending.created_at)
                .num_seconds();
            if !(0..=PROCESS_CONFIRMATION_TTL_SECS).contains(&age) {
                return Err("PROCESS_CONFIRMATION_REQUIRED:确认令牌无效或已过期".to_string());
            }
            pending
        };

        let summary = process_manager::find_process_summary(pid)
            .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
        if !summary.name.eq_ignore_ascii_case(expected_name) {
            return Err("PROCESS_SNAPSHOT_MISMATCH:进程名不匹配".to_string());
        }
        if let Some(expected) = expected_cwd {
            match summary.working_directory.as_deref() {
                Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
                _ => return Err("PROCESS_SNAPSHOT_MISMATCH:工作目录不匹配".to_string()),
            }
        }
        let start_time = process_manager::process_start_time_secs(pid)
            .ok_or_else(|| "PROCESS_NOT_FOUND:进程不存在".to_string())?;
        let current_digest = process_confirmation_digest(
            pid,
            start_time,
            &summary.name,
            summary.working_directory.as_deref(),
            mode,
        );
        if current_digest != pending.binding_digest {
            return Err("PROCESS_CONFIRMATION_REQUIRED:目标进程已变化，请重新确认".to_string());
        }
        Ok(())
    }
}

fn process_confirmation_digest(
    pid: u32,
    start_time: u64,
    name: &str,
    cwd: Option<&str>,
    mode: &str,
) -> String {
    let binding = format!(
        "{}|{}|{}|{}|{}",
        pid,
        start_time,
        name.to_lowercase(),
        cwd.unwrap_or("").to_lowercase(),
        mode.to_lowercase()
    );
    let mut hasher = Sha256::new();
    hasher.update(binding.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_is_single_use() {
        let state = AppState::new();
        let (token, _) = state
            .issue_confirmation("p", "echo ok", ".", "frontend")
            .unwrap();
        state
            .consume_confirmation(&token, "p", "echo ok", ".", "frontend")
            .unwrap();
        assert!(state
            .consume_confirmation(&token, "p", "echo ok", ".", "frontend")
            .is_err());
    }

    #[test]
    fn pending_confirmation_limit_evicts_oldest_entries() {
        #[derive(Clone)]
        struct Item(i64);

        let mut map = HashMap::new();
        map.insert("old".to_string(), Item(1));
        map.insert("new".to_string(), Item(2));
        make_room_for_pending(&mut map, 2, |item| item.0);
        assert_eq!(map.len(), 1);
        assert!(!map.contains_key("old"));
        assert!(map.contains_key("new"));
    }

    #[test]
    fn launch_confirmations_are_bounded() {
        let state = AppState::new();
        for index in 0..(MAX_PENDING_LAUNCH_CONFIRMATIONS + 16) {
            state
                .issue_confirmation(&format!("profile-{index}"), "echo ok", ".", "frontend")
                .unwrap();
        }
        assert!(state.confirmations.lock().unwrap().len() <= MAX_PENDING_LAUNCH_CONFIRMATIONS);
    }

    #[test]
    fn process_confirmation_digest_binds_identity_and_mode() {
        let normal = process_confirmation_digest(42, 100, "node.exe", Some("C:\\work"), "normal");
        let force = process_confirmation_digest(42, 100, "node.exe", Some("C:\\work"), "force");
        let reused = process_confirmation_digest(42, 101, "node.exe", Some("C:\\work"), "normal");
        assert_ne!(normal, force);
        assert_ne!(normal, reused);
    }
}
