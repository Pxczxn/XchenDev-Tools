use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DTO_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimeMode {
    CommandProcess,
    WindowsService,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimeItemState {
    Discovered,
    Configured,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeItem {
    pub dto_version: u32,
    pub id: String,
    pub name: String,
    pub runtime_mode: RuntimeMode,
    pub state: RuntimeItemState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    Succeeded,
    Rejected,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub dto_version: u32,
    pub status: OperationStatus,
    pub reason_code: Option<String>,
    pub message: Option<String>,
}

impl OperationResult {
    pub fn succeeded(message: impl Into<String>) -> Self {
        Self {
            dto_version: DTO_VERSION,
            status: OperationStatus::Succeeded,
            reason_code: None,
            message: Some(message.into()),
        }
    }

    pub fn rejected(code: &str, message: impl Into<String>) -> Self {
        Self {
            dto_version: DTO_VERSION,
            status: OperationStatus::Rejected,
            reason_code: Some(code.to_string()),
            message: Some(message.into()),
        }
    }

    pub fn failed(code: &str, message: impl Into<String>) -> Self {
        Self {
            dto_version: DTO_VERSION,
            status: OperationStatus::Failed,
            reason_code: Some(code.to_string()),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub app_version: String,
    pub platform: String,
    pub ipc_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DetectionSource {
    ManualOverride,
    NativeCommand,
    EnvironmentVariable,
    Registry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentCandidate {
    pub runtime_kind: String,
    pub source: DetectionSource,
    pub executable_path: Option<String>,
    pub resolved_path: Option<String>,
    pub version: Option<String>,
    pub validation_status: ValidationStatus,
    pub validation_reason: Option<String>,
    pub is_user_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionDecision {
    pub is_protected: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub pid: u32,
    pub name: String,
    pub working_directory: Option<String>,
    pub command_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortOccupancy {
    pub protocol: String,
    pub port: u16,
    pub listen_address: String,
    pub process: ProcessSummary,
    pub protection: ProtectionDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryProcessMatch {
    pub match_level: u8,
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub working_directory: Option<String>,
    pub command_line: Option<String>,
    pub ports: Vec<u16>,
    pub protection: ProtectionDecision,
    pub snapshot_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TechnologyStack {
    Node,
    Maven,
    Gradle,
    Python,
    Php,
    Rust,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CandidateStatus {
    Ready,
    NeedsConfirmation,
    Conflict,
    EvidenceOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnologyCandidate {
    pub id: String,
    pub directory: String,
    pub evidence_file: String,
    pub stack: TechnologyStack,
    pub status: CandidateStatus,
    pub suggested_command: Option<String>,
    pub scripts: Option<Vec<String>>,
    pub conflict_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectScanResult {
    pub root_path: String,
    pub scan_depth: u8,
    pub candidates: Vec<TechnologyCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessRole {
    Frontend,
    Backend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchProfile {
    pub profile_id: String,
    pub project_id: String,
    pub process_role: ProcessRole,
    pub working_directory: String,
    pub command: String,
    pub source_candidate_id: Option<String>,
    pub user_modified: bool,
    pub port_hint: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfirmation {
    pub confirmation_token: String,
    pub profile_id: String,
    pub binding_summary: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LaunchSessionState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSessionInfo {
    pub launch_session_id: String,
    pub profile_id: String,
    pub pid: Option<u32>,
    pub state: LaunchSessionState,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServiceKind {
    Mysql,
    Redis,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WindowsServiceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Paused,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsServiceInfo {
    pub service_name: String,
    pub display_name: String,
    pub status: WindowsServiceStatus,
    pub kind: ServiceKind,
    pub can_control: bool,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
    #[serde(default)]
    pub extra_protected_process_names: Vec<String>,
    #[serde(default)]
    pub detection_path_hints: HashMap<String, String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 不在环境管理页检测或展示的运行时 id（如 php）
    #[serde(default)]
    pub disabled_runtime_kinds: Vec<String>,
    /// 基础服务页要发现与管理的类型（mysql、redis）
    #[serde(default = "default_managed_service_kinds")]
    pub managed_service_kinds: Vec<String>,
    /// 按类型指定 Windows 服务名（自动匹配不到时使用），如 mysql -> MySQL80
    #[serde(default)]
    pub managed_service_name_hints: HashMap<String, String>,
}

fn default_managed_service_kinds() -> Vec<String> {
    vec!["mysql".to_string(), "redis".to_string()]
}

fn default_log_retention_days() -> u32 {
    7
}

fn default_theme() -> String {
    "dark".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            log_retention_days: default_log_retention_days(),
            extra_protected_process_names: vec![],
            detection_path_hints: HashMap::new(),
            theme: default_theme(),
            disabled_runtime_kinds: vec![],
            managed_service_kinds: default_managed_service_kinds(),
            managed_service_name_hints: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: String,
    pub action: String,
    pub target: String,
    pub result: OperationStatus,
    pub reason_code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentError {
    pub timestamp: String,
    pub code: String,
    pub message: String,
    pub source: String,
}
