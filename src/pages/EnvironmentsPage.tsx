import { useCallback, useEffect, useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  getAppSettings,
  listEnvironmentCandidates,
  saveManualOverride,
} from "../ipc/client";
import type { EnvironmentCandidate } from "../ipc/types";
import {
  formatDisplayPath,
  formatRuntimeKind,
  formatVersionText,
} from "../lib/formatDisplay";
import {
  sortEnvironmentCandidates,
  visibleEnvironmentCandidates,
} from "../lib/environmentSort";
import { enabledRuntimeKindIds } from "../lib/runtimeKinds";
import { labelErrorText, labelStatus } from "../lib/statusLabels";

function statusClass(status: string): string {
  const s = status.toUpperCase();
  if (s === "VALID") return "env-badge ok";
  if (s === "INVALID") return "env-badge err";
  return "env-badge muted";
}

function CandidateRow({ candidate }: { candidate: EnvironmentCandidate }) {
  const path = formatDisplayPath(
    candidate.resolved_path ?? candidate.executable_path,
  );
  const version = formatVersionText(candidate.version);
  const fullPath = candidate.resolved_path ?? candidate.executable_path ?? "";

  return (
    <div className="env-candidate">
      <div className="env-candidate-head">
        <span className={statusClass(candidate.validation_status)}>
          {labelStatus(candidate.validation_status)}
        </span>
        <span className="env-source">{labelStatus(candidate.source)}</span>
        {candidate.is_user_configured && (
          <span className="env-tag">用户配置</span>
        )}
      </div>
      <div className="env-path" title={fullPath}>{path}</div>
      <div className="env-meta">
        <span className="env-meta-label">版本</span>
        <span className="env-version" title={candidate.version ?? ""}>
          {version}
        </span>
      </div>
      {candidate.validation_reason && (
        <div className="env-reason">{candidate.validation_reason}</div>
      )}
    </div>
  );
}

export function EnvironmentsPage() {
  const [candidates, setCandidates] = useState<EnvironmentCandidate[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manualKind, setManualKind] = useState("node");
  const [manualPath, setManualPath] = useState("");
  const [enabledKinds, setEnabledKinds] = useState<string[]>([
    "java",
    "python",
    "node",
    "php",
    "rust",
  ]);

  const load = useCallback(async (forceRefresh = false) => {
    setLoading(true);
    setError(null);
    try {
      const settings = await getAppSettings();
      setEnabledKinds(
        enabledRuntimeKindIds(settings.disabled_runtime_kinds ?? []),
      );
      const data = await listEnvironmentCandidates([], forceRefresh);
      setCandidates(visibleEnvironmentCandidates(data));
    } catch (e) {
      setError(labelErrorText(String(e)));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(false);
  }, [load]);

  useEffect(() => {
    if (!enabledKinds.includes(manualKind) && enabledKinds.length > 0) {
      setManualKind(enabledKinds[0]);
    }
  }, [enabledKinds, manualKind]);

  const manualDisabled = enabledKinds.length === 0;

  async function onSaveManual() {
    if (manualDisabled || !manualPath.trim()) return;
    try {
      await saveManualOverride(manualKind, manualPath);
      await load(true);
    } catch (e) {
      setError(labelErrorText(String(e)));
    }
  }

  const grouped = candidates.reduce<Record<string, EnvironmentCandidate[]>>(
    (acc, c) => {
      (acc[c.runtime_kind] ??= []).push(c);
      return acc;
    },
    {},
  );

  for (const kind of Object.keys(grouped)) {
    grouped[kind] = sortEnvironmentCandidates(grouped[kind]);
  }

  const runtimeOrder = enabledKinds;
  const sortedGroups = Object.entries(grouped).sort(([a], [b]) => {
    const ia = runtimeOrder.indexOf(a);
    const ib = runtimeOrder.indexOf(b);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
  });

  return (
    <>
      <PageHeader
        title="环境管理"
        description="检测本机运行时路径与版本；可在「设置」中勾选要显示的环境类型"
        actions={
          <button type="button" className="btn-sm" onClick={() => void load(true)} disabled={loading}>
            {loading ? "扫描中…" : "重新检测"}
          </button>
        }
      />

      {(loading || error) && (
        <div className="feedback-banner">
          {loading && "正在扫描本机运行时…"}
          {error && <span className="status-pill err">{error}</span>}
        </div>
      )}

      {sortedGroups.length === 0 && !loading && (
        <div className="card">
          <div className="empty">
            {manualDisabled
              ? "未启用任何运行时类型，请先在「设置」中勾选。"
              : "无候选，请手动指定路径。"}
          </div>
        </div>
      )}

      {sortedGroups.map(([kind, list]) => (
        <section key={kind} className="card env-runtime-block">
          <div className="card-header">
            <h3>{formatRuntimeKind(kind)}</h3>
            <span className="card-meta">{list.length} 个候选</span>
          </div>
          <div className="card-body env-candidate-list">
            {list.map((c, i) => (
              <CandidateRow key={`${kind}-${i}-${c.executable_path ?? i}`} candidate={c} />
            ))}
          </div>
        </section>
      ))}

      <div className="card">
        <div className="card-header">
          <h3>手动指定路径</h3>
        </div>
        <div className="card-body">
        <p className="muted env-hint">
          {manualDisabled
            ? "未启用任何运行时类型，手动覆盖暂不可用。"
            : "当自动检测不准确时，可指定可执行文件路径并保存为优先候选。"}
        </p>
        <div className="form-row env-manual-form">
          <select
            value={manualKind}
            onChange={(e) => setManualKind(e.target.value)}
            disabled={manualDisabled}
          >
            {enabledKinds.map((id) => (
              <option key={id} value={id}>{formatRuntimeKind(id)}</option>
            ))}
          </select>
          <input
            className="env-path-input"
            placeholder="C:\path\to\executable.exe"
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            disabled={manualDisabled}
          />
          <button
            type="button"
            onClick={onSaveManual}
            disabled={manualDisabled || loading || !manualPath.trim()}
          >
            保存并验证
          </button>
        </div>
        </div>
      </div>
    </>
  );
}
