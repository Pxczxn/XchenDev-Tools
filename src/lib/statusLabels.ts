/** 将后端状态码 / 枚举值映射为界面中文文案 */

const STATUS_LABELS: Record<string, string> = {
  // IPC / 健康
  ONLINE: "在线",
  OFFLINE: "离线",
  DEGRADED: "降级",

  // 操作结果
  SUCCEEDED: "成功",
  REJECTED: "已拒绝",
  FAILED: "失败",
  UNKNOWN: "未知",

  // 环境检测
  VALID: "可用",
  INVALID: "不可用",
  MANUAL_OVERRIDE: "手动指定",
  NATIVE_COMMAND: "原生命令",
  ENVIRONMENT_VARIABLE: "环境变量",
  REGISTRY: "注册表",

  // 运行项
  COMMAND_PROCESS: "命令进程",
  WINDOWS_SERVICE: "Windows 服务",
  DISCOVERED: "已发现",
  CONFIGURED: "已配置",
  STARTING: "启动中",
  RUNNING: "运行中",
  STOPPING: "停止中",
  STOPPED: "已停止",
  BLOCKED: "已阻塞",

  // 服务
  MYSQL: "MySQL",
  REDIS: "Redis",
  PAUSED: "已暂停",

  // 项目 / 技术栈
  NODE: "Node.js",
  MAVEN: "Maven",
  GRADLE: "Gradle",
  PYTHON: "Python",
  PHP: "PHP",
  RUST: "Rust",
  READY: "就绪",
  NEEDS_CONFIRMATION: "需确认",
  CONFLICT: "冲突",
  EVIDENCE_ONLY: "仅证据",

  // 角色
  FRONTEND: "前端",
  BACKEND: "后端",
  frontend: "前端",
  backend: "后端",

  // 日志流
  stdout: "标准输出",
  stderr: "标准错误",
  exit: "退出",

  // 平台
  windows: "Windows",
  linux: "Linux",
  macos: "macOS",

  // 错误码
  IPC_NOT_READY: "IPC 未就绪",
  PATH_NOT_FOUND: "路径不存在",
  EXECUTABLE_INVALID: "可执行文件无效",
  RUNTIME_KIND_INVALID: "运行时类型无效",
  DETECT_PERMISSION_DENIED: "检测权限不足",
  DETECT_IO_ERROR: "检测 IO 错误",
  PORT_INVALID: "端口无效",
  PORT_QUERY_FAILED: "端口查询失败",
  PROCESS_PROTECTED: "进程受保护",
  PROCESS_NOT_FOUND: "进程不存在",
  PROCESS_QUERY_FAILED: "进程查询失败",
  PROCESS_SNAPSHOT_MISMATCH: "进程快照不匹配",
  PROCESS_CONFIRMATION_ISSUE_FAILED: "进程确认签发失败",
  PROCESS_CONFIRMATION_REQUIRED: "需要重新确认目标进程",
  TERMINATE_MODE_INVALID: "终止模式无效",
  TERMINATE_DENIED: "终止被拒绝",
  TERMINATE_FAILED: "终止失败",
  DIRECTORY_INVALID: "目录无效",
  PROJECT_PATH_INVALID: "项目路径无效",
  PROJECT_ROOT_INVALID: "项目根目录无效",
  PROJECT_NOT_FOUND: "项目不存在",
  PROJECT_RUNNING: "项目仍在运行",
  PROJECT_SCAN_DENIED: "项目扫描被拒绝",
  PROJECT_SCAN_FAILED: "项目扫描失败",
  WORKDIR_INVALID: "工作目录无效",
  WORKDIR_OUTSIDE_PROJECT: "工作目录超出项目范围",
  COMMAND_POLICY_REJECTED: "命令策略拒绝",
  PROFILE_INVALID: "配置无效",
  PROFILE_NOT_FOUND: "配置不存在",
  PROFILE_RUNNING: "启动配置仍在运行",
  CONFIG_IMPORT_BLOCKED_ACTIVE_SESSIONS: "运行中无法导入配置",
  CONFIG_TRANSACTION_LOCK_FAILED: "配置事务锁失败",
  CONFIG_ROLLBACK_PERSIST_FAILED: "配置回滚落盘失败",
  SETTINGS_INVALID: "设置无效",
  CONFIRMATION_ISSUE_FAILED: "确认签发失败",
  LAUNCH_CONFIRMATION_REQUIRED: "需要启动确认",
  LAUNCH_START_FAILED: "启动失败",
  LAUNCH_LIFECYCLE_LOCK_FAILED: "启动关系操作失败",
  LAUNCH_ALREADY_RUNNING: "已有运行会话",
  LAUNCH_SESSION_NOT_FOUND: "会话不存在",
  LAUNCH_PROCESS_IDENTITY_MISMATCH: "启动进程身份已变化",
  LAUNCH_STOP_FAILED: "停止失败",
  SERVICE_NOT_FOUND: "服务不存在或不在管理范围",
  SERVICE_QUERY_FAILED: "服务查询失败",
  SERVICE_CONFIRMATION_ISSUE_FAILED: "服务确认签发失败",
  SERVICE_CONFIRMATION_REQUIRED: "需要重新确认服务操作",
  SERVICE_CONTROL_FAILED: "服务控制失败",
  SERVICE_CONTROL_DENIED: "服务控制被拒绝",
  SERVICE_CONTROL_TIMEOUT: "等待服务状态变化超时",
  SERVICE_ACTION_INVALID: "服务操作无效",
};

function toScreamingSnake(value: string): string {
  return value
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toUpperCase();
}

export function labelStatus(code: string | undefined | null): string {
  if (!code) return "—";
  const trimmed = code.trim();
  if (!trimmed) return "—";
  const snake = toScreamingSnake(trimmed);
  return (
    STATUS_LABELS[trimmed] ??
    STATUS_LABELS[trimmed.toUpperCase()] ??
    STATUS_LABELS[snake] ??
    trimmed
  );
}

/** 解析 `CODE:说明` 或纯错误码 */
export function labelErrorText(raw: string | undefined | null): string {
  if (!raw) return "";
  const text = String(raw);
  const colon = text.indexOf(":");
  if (colon > 0) {
    const code = text.slice(0, colon);
    const detail = text.slice(colon + 1);
    const codeLabel = labelStatus(code);
    if (detail && detail !== codeLabel) {
      return `${codeLabel}：${detail}`;
    }
    return detail || codeLabel;
  }
  return labelStatus(text);
}

export function labelPlatform(platform: string): string {
  return labelStatus(platform.toLowerCase());
}

export function formatOperationMessage(
  status: string,
  message?: string | null,
  reasonCode?: string | null,
): string {
  const parts: string[] = [labelStatus(status)];
  if (reasonCode) {
    parts.push(labelStatus(reasonCode));
  }
  if (message) {
    parts.push(message);
  }
  return parts.filter(Boolean).join(" · ");
}
