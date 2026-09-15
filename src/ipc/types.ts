export type OperationStatus = "SUCCEEDED" | "REJECTED" | "FAILED" | "UNKNOWN";

export interface OperationResult {
  dto_version: number;
  status: OperationStatus;
  reason_code?: string;
  message?: string;
}

export interface HealthCheckResponse {
  app_version: string;
  platform: string;
  ipc_status: string;
}

export interface IpcError {
  code: string;
  message: string;
}

export interface EnvironmentCandidate {
  runtime_kind: string;
  source: string;
  executable_path?: string;
  resolved_path?: string;
  version?: string;
  validation_status: string;
  validation_reason?: string;
  is_user_configured: boolean;
}

export interface PortOccupancy {
  protocol: string;
  port: number;
  listen_address: string;
  process: {
    pid: number;
    name: string;
    working_directory?: string;
    command_line?: string;
  };
  protection: { is_protected: boolean; reason?: string };
}

export interface ProcessTerminationConfirmation {
  confirmation_token: string;
  binding_summary: string;
  expires_at: string;
}

export interface DirectoryProcessMatch {
  match_level: number;
  pid: number;
  parent_pid?: number;
  name: string;
  working_directory?: string;
  command_line?: string;
  ports: number[];
  protection: { is_protected: boolean; reason?: string };
  snapshot_digest: string;
}

export interface TechnologyCandidate {
  id: string;
  directory: string;
  evidence_file: string;
  stack: string;
  status: string;
  suggested_command?: string;
  scripts?: string[];
  conflict_group?: string;
}

export interface ProjectScanResult {
  root_path: string;
  scan_depth: number;
  candidates: TechnologyCandidate[];
}

export interface ProjectInfo {
  project_id: string;
  name: string;
  root_path: string;
  created_at: string;
  updated_at: string;
}

export interface LaunchProfile {
  profile_id: string;
  project_id: string;
  process_role: string;
  working_directory: string;
  command: string;
  source_candidate_id?: string;
  user_modified: boolean;
  port_hint?: number;
}

export interface LaunchConfirmation {
  confirmation_token: string;
  profile_id: string;
  binding_summary: string;
  expires_at: string;
}

export type ThemeMode = "dark" | "light";

export interface AppSettings {
  log_retention_days: number;
  extra_protected_process_names: string[];
  detection_path_hints: Record<string, string>;
  theme: ThemeMode;
  /** 不在环境管理页检测或展示的运行时 id */
  disabled_runtime_kinds: string[];
  /** 基础服务页要管理的类型：mysql、redis */
  managed_service_kinds: string[];
  /** 类型 -> Windows 服务名，自动发现不到时填写 */
  managed_service_name_hints: Record<string, string>;
}

export interface AuditEvent {
  timestamp: string;
  action: string;
  target: string;
  result: string;
  reason_code?: string;
  message?: string;
}

export interface RecentError {
  timestamp: string;
  code: string;
  message: string;
  source: string;
}

export interface WindowsServiceInfo {
  service_name: string;
  display_name: string;
  status: string;
  kind: string;
  can_control: boolean;
  status_reason?: string;
}

export interface RuntimeItem {
  dto_version: number;
  id: string;
  name: string;
  runtime_mode: string;
  state: string;
}

export interface LaunchSessionInfo {
  launch_session_id: string;
  profile_id: string;
  pid?: number;
  state: string;
  exit_code?: number;
}
