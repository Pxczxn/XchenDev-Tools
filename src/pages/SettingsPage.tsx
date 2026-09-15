import { open, save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import {
  exportAppConfig,
  exportAppConfigToPath,
  getAppSettings,
  importAppConfig,
  importAppConfigFromPath,
  listAuditEvents,
  listDefaultProtectedProcesses,
  saveAppSettings,
} from "../ipc/client";
import { PageHeader } from "../components/PageHeader";
import { useTheme } from "../context/ThemeContext";
import type { AppSettings, AuditEvent, ThemeMode } from "../ipc/types";
import { formatOperationMessage, labelErrorText, labelStatus } from "../lib/statusLabels";
import { formatRuntimeKind } from "../lib/formatDisplay";
import {
  formatManagedServiceKind,
  MANAGED_SERVICE_KIND_IDS,
  normalizeManagedServiceKinds,
  sanitizeManagedServiceKinds,
} from "../lib/managedServices";
import { RUNTIME_KIND_IDS } from "../lib/runtimeKinds";
import { normalizeTheme } from "../lib/theme";

const DEFAULT_SETTINGS: AppSettings = {
  log_retention_days: 7,
  extra_protected_process_names: [],
  detection_path_hints: {},
  theme: "dark",
  disabled_runtime_kinds: [],
  managed_service_kinds: ["mysql", "redis"],
  managed_service_name_hints: {},
};

export function SettingsPage() {
  const { theme, setTheme } = useTheme();
  const [configDir, setConfigDir] = useState("");
  const [configFile, setConfigFile] = useState("");
  const [importText, setImportText] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [protectedText, setProtectedText] = useState("");
  const [hintsText, setHintsText] = useState("");
  const [serviceHintsText, setServiceHintsText] = useState("");
  const [auditEvents, setAuditEvents] = useState<AuditEvent[]>([]);
  const [defaultProtected, setDefaultProtected] = useState<string[]>([]);
  const [operationBusy, setOperationBusy] = useState(false);
  const operationBusyRef = useRef(false);

  useEffect(() => {
    invoke<[string, string]>("get_config_paths")
      .then(([dir, file]) => {
        setConfigDir(dir);
        setConfigFile(file);
      })
      .catch(() => {
        setConfigDir("（应用同目录）/config");
        setConfigFile("（应用同目录）/config/config.json");
      });
    getAppSettings()
      .then((s) => {
        setSettings({
          ...s,
          disabled_runtime_kinds: s.disabled_runtime_kinds ?? [],
          managed_service_kinds: normalizeManagedServiceKinds(
            s.managed_service_kinds,
          ),
        });
        setProtectedText(s.extra_protected_process_names.join("\n"));
        setHintsText(
          Object.entries(s.detection_path_hints)
            .map(([k, v]) => `${k}=${v}`)
            .join("\n"),
        );
        setServiceHintsText(
          Object.entries(s.managed_service_name_hints ?? {})
            .map(([k, v]) => `${k}=${v}`)
            .join("\n"),
        );
      })
      .catch(() => undefined);
    listAuditEvents(15).then(setAuditEvents).catch(() => undefined);
    listDefaultProtectedProcesses()
      .then(setDefaultProtected)
      .catch(() => undefined);
  }, []);

  async function reloadSettingsFromConfig() {
    const s = await getAppSettings();
    setSettings({
      ...s,
      disabled_runtime_kinds: s.disabled_runtime_kinds ?? [],
      managed_service_kinds: normalizeManagedServiceKinds(s.managed_service_kinds),
    });
    setProtectedText(s.extra_protected_process_names.join("\n"));
    setHintsText(
      Object.entries(s.detection_path_hints)
        .map(([k, v]) => `${k}=${v}`)
        .join("\n"),
    );
    setServiceHintsText(
      Object.entries(s.managed_service_name_hints ?? {})
        .map(([k, v]) => `${k}=${v}`)
        .join("\n"),
    );
    await setTheme(normalizeTheme(s.theme));
  }

  async function runConfigOperation(operation: () => Promise<void>) {
    if (operationBusyRef.current) return;
    operationBusyRef.current = true;
    setOperationBusy(true);
    try {
      await operation();
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    } finally {
      operationBusyRef.current = false;
      setOperationBusy(false);
    }
  }

  async function onSaveSettings() {
    await runConfigOperation(async () => {
      const extra = protectedText
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean);
      const hints: Record<string, string> = {};
      for (const line of hintsText.split(/\r?\n/)) {
        const trimmed = line.trim();
        if (!trimmed || !trimmed.includes("=")) continue;
        const [k, ...rest] = trimmed.split("=");
        hints[k.trim()] = rest.join("=").trim();
      }
      const serviceHints: Record<string, string> = {};
      for (const line of serviceHintsText.split(/\r?\n/)) {
        const trimmed = line.trim();
        if (!trimmed || !trimmed.includes("=")) continue;
        const [k, ...rest] = trimmed.split("=");
        const key = k.trim().toLowerCase();
        if (key === "mysql" || key === "redis") {
          serviceHints[key] = rest.join("=").trim();
        }
      }
      const next: AppSettings = {
        ...settings,
        theme,
        extra_protected_process_names: extra,
        detection_path_hints: hints,
        managed_service_kinds: sanitizeManagedServiceKinds(
          settings.managed_service_kinds,
        ),
        managed_service_name_hints: serviceHints,
      };
      const result = await saveAppSettings(next);
      if (result.status === "SUCCEEDED") {
        await reloadSettingsFromConfig();
      }
      setMessage(
        formatOperationMessage(result.status, result.message, result.reason_code),
      );
      setAuditEvents(await listAuditEvents(15));
    });
  }

  async function onExportClipboard() {
    await runConfigOperation(async () => {
      const json = await exportAppConfig();
      await navigator.clipboard.writeText(json);
      setMessage("配置已复制到剪贴板");
    });
  }

  async function onExportFile() {
    await runConfigOperation(async () => {
      const path = await save({
        defaultPath: "config.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const result = await exportAppConfigToPath(path);
      setMessage(
        formatOperationMessage(result.status, result.message, result.reason_code),
      );
    });
  }

  async function onImportFromFile() {
    await runConfigOperation(async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path || typeof path !== "string") return;
      const result = await importAppConfigFromPath(path);
      if (result.status === "SUCCEEDED") {
        await reloadSettingsFromConfig();
      }
      setMessage(
        formatOperationMessage(result.status, result.message, result.reason_code),
      );
    });
  }

  async function onImportPaste() {
    if (!importText.trim()) return;
    const content = importText;
    await runConfigOperation(async () => {
      const result = await importAppConfig(content);
      if (result.status === "SUCCEEDED") {
        await reloadSettingsFromConfig();
      }
      setMessage(
        formatOperationMessage(result.status, result.message, result.reason_code),
      );
      setImportText("");
    });
  }

  return (
    <>
      <PageHeader
        title="设置"
        description="运行策略、进程保护与配置导入导出"
      />
      <div className="card">
        <div className="card-header">
          <h3>配置存储</h3>
        </div>
        <div className="card-body">
        <p className="muted">
          默认目录为应用同目录下的 <code>config</code>，主文件为{" "}
          <code>config.json</code>。
        </p>
        {configDir && (
          <>
            <p className="muted">目录：{configDir}</p>
            <p className="muted">文件：{configFile}</p>
          </>
        )}
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h3>运行策略</h3>
        </div>
        <div className="card-body">
        <div className="form-row">
          <label className="muted">界面主题</label>
          <select
            value={theme}
            onChange={(e) => void setTheme(e.target.value as ThemeMode)}
            disabled={operationBusy}
          >
            <option value="dark">深色</option>
            <option value="light">浅色</option>
          </select>
          <span className="muted">写入 config.json，导入/导出时一并保留</span>
        </div>
        <div className="form-row">
          <label className="muted">日志保留天数</label>
          <input
            type="number"
            min={1}
            max={365}
            value={settings.log_retention_days}
            onChange={(e) =>
              setSettings({
                ...settings,
                log_retention_days: Number(e.target.value) || 7,
              })
            }
            className="input-narrow"
            disabled={operationBusy}
          />
        </div>
        <p className="muted env-hint">
          系统默认保护进程（内置，不可编辑；PID ≤ 4 的进程同样受保护）
        </p>
        <textarea
          className="settings-import-area settings-readonly-area"
          value={defaultProtected.join("\n")}
          readOnly
          tabIndex={-1}
        />
        <p className="muted env-hint">额外受保护进程名（每行一个，保存后生效）</p>
        <textarea
          className="settings-import-area"
          value={protectedText}
          onChange={(e) => setProtectedText(e.target.value)}
          placeholder="例如：my-service.exe"
          disabled={operationBusy}
        />
        <p className="muted env-hint">检测路径提示（每行 runtime=path，如 node=C:\node\node.exe）</p>
        <textarea
          className="settings-import-area"
          value={hintsText}
          onChange={(e) => setHintsText(e.target.value)}
          disabled={operationBusy}
        />
        <p className="muted env-hint">
          环境管理页显示的运行时（取消勾选后不再检测、不展示该类型）
        </p>
        <div className="runtime-kind-toggles">
          {RUNTIME_KIND_IDS.map((id) => {
            const enabled = !settings.disabled_runtime_kinds.includes(id);
            return (
              <label key={id} className="runtime-kind-toggle">
                <input
                  type="checkbox"
                  checked={enabled}
                  disabled={operationBusy}
                  onChange={(e) => {
                    const nextDisabled = e.target.checked
                      ? settings.disabled_runtime_kinds.filter((k) => k !== id)
                      : [...settings.disabled_runtime_kinds, id];
                    setSettings({
                      ...settings,
                      disabled_runtime_kinds: nextDisabled,
                    });
                  }}
                />
                <span>{formatRuntimeKind(id)}</span>
              </label>
            );
          })}
        </div>
        <p className="muted env-hint">
          基础服务页要管理的 Windows 服务类型（取消勾选后不再发现、不展示）
        </p>
        <div className="runtime-kind-toggles">
          {MANAGED_SERVICE_KIND_IDS.map((id) => {
            const enabled = settings.managed_service_kinds.includes(id);
            return (
              <label key={id} className="runtime-kind-toggle">
                <input
                  type="checkbox"
                  checked={enabled}
                  disabled={operationBusy}
                  onChange={(e) => {
                    const next = e.target.checked
                      ? sanitizeManagedServiceKinds([
                          ...settings.managed_service_kinds,
                          id,
                        ])
                      : settings.managed_service_kinds.filter((k) => k !== id);
                    setSettings({
                      ...settings,
                      managed_service_kinds: next,
                    });
                  }}
                />
                <span>{formatManagedServiceKind(id)}</span>
              </label>
            );
          })}
        </div>
        <p className="muted env-hint">
          服务名映射（每行 kind=Windows服务名；自动发现不到时填写，如 mysql=MySQL80）
        </p>
        <textarea
          className="settings-import-area"
          value={serviceHintsText}
          onChange={(e) => setServiceHintsText(e.target.value)}
          placeholder={"mysql=MySQL80\nredis=redis"}
          disabled={operationBusy}
        />
        <div className="form-row env-manual-form">
          <button type="button" onClick={onSaveSettings} disabled={operationBusy}>
            {operationBusy ? "处理中…" : "保存设置"}
          </button>
        </div>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h3>配置导入 / 导出</h3>
        </div>
        <div className="card-body">
        <div className="form-row">
          <button type="button" onClick={onExportClipboard} disabled={operationBusy}>复制到剪贴板</button>
          <button type="button" className="secondary" onClick={onExportFile} disabled={operationBusy}>导出到文件</button>
          <button type="button" className="secondary" onClick={onImportFromFile} disabled={operationBusy}>从文件导入</button>
        </div>
        <textarea
          className="settings-import-area"
          placeholder="或粘贴 JSON 配置后点击导入"
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          disabled={operationBusy}
        />
        <div className="form-row env-manual-form">
          <button type="button" onClick={onImportPaste} disabled={operationBusy || !importText.trim()}>
            {operationBusy ? "处理中…" : "导入粘贴内容"}
          </button>
        </div>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h3>最近审计</h3>
        </div>
        <div className="card-body">
        {auditEvents.length === 0 && (
          <p className="muted">暂无审计记录。</p>
        )}
        <div className="env-candidate-list">
          {auditEvents.map((e, i) => (
            <div key={`${e.timestamp}-${i}`} className="env-candidate">
              <div className="env-candidate-head">
                <span className="env-badge muted">{labelStatus(e.result)}</span>
                <span className="env-source">{e.action}</span>
              </div>
              <div className="env-meta">
                <span className="env-meta-label">目标</span>
                <span className="env-version">{e.target}</span>
              </div>
              {e.message && <div className="env-reason">{e.message}</div>}
            </div>
          ))}
        </div>
        </div>
      </div>

      {message && <div className="feedback-banner">{message}</div>}
    </>
  );
}