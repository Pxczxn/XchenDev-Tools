import { invoke } from "@tauri-apps/api/core";
import type {
  DirectoryProcessMatch,
  EnvironmentCandidate,
  HealthCheckResponse,
  LaunchConfirmation,
  LaunchProfile,
  LaunchSessionInfo,
  OperationResult,
  AppSettings,
  AuditEvent,
  RecentError,
  RuntimeItem,
  WindowsServiceInfo,
  PortOccupancy,
  ProjectInfo,
  ProjectScanResult,
} from "./types";

function parseError(err: unknown): { code: string; message: string } {
  if (typeof err === "string") {
    return { code: "UNKNOWN", message: err };
  }
  if (err && typeof err === "object") {
    const o = err as Record<string, unknown>;
    if (typeof o.code === "string") {
      return { code: o.code, message: String(o.message ?? o.code) };
    }
  }
  return { code: "UNKNOWN", message: String(err) };
}

export async function listRuntimeItems(): Promise<RuntimeItem[]> {
  return invoke("list_runtime_items_safe");
}

export async function listLaunchSessions(): Promise<LaunchSessionInfo[]> {
  return invoke("list_active_launch_sessions");
}

export async function listManagedServices(): Promise<WindowsServiceInfo[]> {
  return invoke("list_managed_services");
}

export async function controlWindowsService(args: {
  serviceName: string;
  action: string;
  confirmationToken: string;
}): Promise<OperationResult> {
  return invoke("control_windows_service", {
    serviceName: args.serviceName,
    action: args.action,
    confirmationToken: args.confirmationToken,
  });
}

export async function getAppSettings(): Promise<AppSettings> {
  return invoke("get_app_settings");
}

export async function saveAppSettings(
  settings: AppSettings,
): Promise<OperationResult> {
  return invoke("save_app_settings", { settings });
}

export async function listAuditEvents(limit = 20): Promise<AuditEvent[]> {
  return invoke("list_audit_events", { limit });
}

export async function listRecentErrors(limit = 10): Promise<RecentError[]> {
  return invoke("list_recent_errors", { limit });
}

export async function listDefaultProtectedProcesses(): Promise<string[]> {
  return invoke("list_default_protected_processes");
}

export async function exportAppConfig(): Promise<string> {
  return invoke("export_app_config");
}

export async function importAppConfig(content: string): Promise<OperationResult> {
  return invoke("import_app_config_safe", { content });
}

export async function importAppConfigFromPath(
  sourcePath: string,
): Promise<OperationResult> {
  return invoke("import_app_config_from_path_safe", { sourcePath });
}

export async function exportAppConfigToPath(
  targetPath: string,
): Promise<OperationResult> {
  return invoke("export_app_config_to_path", { targetPath });
}

export async function healthCheck(): Promise<HealthCheckResponse> {
  try {
    return await invoke<HealthCheckResponse>("health_check");
  } catch (e) {
    const parsed = parseError(e);
    throw parsed;
  }
}

export async function listEnvironmentCandidates(
  runtimeKinds: string[] = [],
  forceRefresh = false,
): Promise<EnvironmentCandidate[]> {
  return invoke("list_environment_candidates", { runtimeKinds, forceRefresh });
}

export async function saveManualOverride(
  runtimeKind: string,
  executablePath: string,
): Promise<EnvironmentCandidate> {
  return invoke("save_manual_override", { runtimeKind, executablePath });
}

export async function inspectPort(
  protocol: string,
  port: number,
): Promise<PortOccupancy[]> {
  return invoke("inspect_port", { protocol, port });
}

export async function terminateProcess(args: {
  pid: number;
  mode: string;
  confirmationToken: string;
  expectedName: string;
  expectedCwd?: string;
}): Promise<OperationResult> {
  return invoke("terminate_process", {
    pid: args.pid,
    mode: args.mode,
    confirmationToken: args.confirmationToken,
    expectedName: args.expectedName,
    expectedCwd: args.expectedCwd ?? null,
  });
}

export async function inspectDirectoryProcesses(
  rootPath: string,
): Promise<DirectoryProcessMatch[]> {
  return invoke("inspect_directory_processes", { rootPath });
}

export async function terminateDirectoryProcess(args: {
  pid: number;
  snapshotDigest: string;
  mode: string;
  confirmationToken: string;
  expectedName: string;
  expectedCwd?: string;
}): Promise<OperationResult> {
  return invoke("terminate_directory_process", {
    pid: args.pid,
    snapshotDigest: args.snapshotDigest,
    mode: args.mode,
    confirmationToken: args.confirmationToken,
    expectedName: args.expectedName,
    expectedCwd: args.expectedCwd ?? null,
  });
}

export async function scanProjectDirectory(
  rootPath: string,
): Promise<ProjectScanResult> {
  return invoke("scan_project_directory", { rootPath });
}

export async function projectIdForPath(rootPath: string): Promise<string> {
  return invoke("project_id_for_path", { rootPath });
}

export async function listProjects(): Promise<ProjectInfo[]> {
  return invoke("list_projects");
}

export async function upsertProject(
  rootPath: string,
  name?: string,
): Promise<ProjectInfo> {
  return invoke("upsert_project", { rootPath, name: name ?? null });
}

export async function removeProject(projectId: string): Promise<OperationResult> {
  return invoke("remove_project", { projectId });
}

export async function saveLaunchProfile(args: {
  projectId: string;
  processRole: string;
  workingDirectory: string;
  command: string;
  sourceCandidateId?: string;
}): Promise<string> {
  return invoke("save_launch_profile_safe", {
    projectId: args.projectId,
    processRole: args.processRole,
    workingDirectory: args.workingDirectory,
    command: args.command,
    sourceCandidateId: args.sourceCandidateId ?? null,
  });
}

export async function removeLaunchProfile(
  profileId: string,
): Promise<OperationResult> {
  return invoke("remove_launch_profile_safe", { profileId });
}

export async function listLaunchProfiles(
  projectId: string,
): Promise<LaunchProfile[]> {
  return invoke("list_launch_profiles", { projectId });
}

export async function issueLaunchConfirmation(
  profileId: string,
): Promise<LaunchConfirmation> {
  return invoke("issue_launch_confirmation_safe", { profileId });
}

export async function startLaunchProfile(
  profileId: string,
  confirmationToken: string,
): Promise<LaunchSessionInfo> {
  return invoke("start_launch_profile_safe", { profileId, confirmationToken });
}

export async function stopLaunchSession(
  launchSessionId: string,
): Promise<OperationResult> {
  return invoke("stop_launch_session", { launchSessionId });
}
