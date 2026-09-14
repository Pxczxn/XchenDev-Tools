use crate::command_runner::CommandRunnerState;
use crate::config_store::ConfigStore;
use crate::domain::EnvironmentCandidate;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const ENV_DETECTION_CACHE_TTL: Duration = Duration::from_secs(120);
pub const LAUNCH_CONFIRMATION_TTL_SECS: i64 = 120;

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

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ConfigStore::new(),
            command_runner: CommandRunnerState::new(),
            confirmations: Mutex::new(HashMap::new()),
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
            !item.consumed
                && Utc::now()
                    .signed_duration_since(item.created_at)
                    .num_seconds()
                    <= LAUNCH_CONFIRMATION_TTL_SECS
        });
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
            || age_seconds < 0
            || age_seconds > LAUNCH_CONFIRMATION_TTL_SECS
            || pending.profile_id != profile_id
            || pending.binding_digest != digest
        {
            map.remove(token);
            return Err("LAUNCH_CONFIRMATION_REQUIRED:确认令牌无效或已过期".to_string());
        }
        map.remove(token);
        Ok(())
    }
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
}
