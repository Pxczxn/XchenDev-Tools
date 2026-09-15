import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { PageHeader } from "../components/PageHeader";
import {
  inspectDirectoryProcesses,
  issueProcessTerminationConfirmation,
  terminateDirectoryProcess,
} from "../ipc/client";
import type { DirectoryProcessMatch } from "../ipc/types";
import { formatDisplayPath } from "../lib/formatDisplay";
import {
  formatOperationMessage,
  labelErrorText,
} from "../lib/statusLabels";

export function ProcessesPage() {
  const [rootPath, setRootPath] = useState("");
  const [rows, setRows] = useState<DirectoryProcessMatch[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function pickDir() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setRootPath(selected);
    }
  }

  async function onScan() {
    if (!rootPath) return;
    setLoading(true);
    setMessage(null);
    try {
      const data = await inspectDirectoryProcesses(rootPath);
      setRows(data);
    } catch (e) {
      setMessage(labelErrorText(String(e)));
      setRows([]);
    } finally {
      setLoading(false);
    }
  }

  async function onTerminate(row: DirectoryProcessMatch, force: boolean) {
    if (row.protection.is_protected) return;
    const mode = force ? "force" : "normal";
    try {
      const confirmation = await issueProcessTerminationConfirmation({
        pid: row.pid,
        mode,
        expectedName: row.name,
        expectedCwd: row.working_directory,
      });
      const ok = window.confirm(`确认${force ? "强制" : ""}终止？\n${confirmation.binding_summary}`);
      if (!ok) return;

      const result = await terminateDirectoryProcess({
        pid: row.pid,
        snapshotDigest: row.snapshot_digest,
        mode,
        confirmationToken: confirmation.confirmation_token,
        expectedName: row.name,
        expectedCwd: row.working_directory,
      });
      setMessage(
        formatOperationMessage(
          result.status,
          result.message,
          result.reason_code,
        ),
      );
      await onScan();
    } catch (e) {
      setMessage(labelErrorText(String(e)));
    }
  }

  return (
    <>
      <PageHeader
        title="进程管理"
        description="按项目目录扫描关联进程，支持受控终止"
      />
      <div className="card toolbar-card env-toolbar">
        <input
          className="env-path-input"
          value={rootPath}
          onChange={(e) => setRootPath(e.target.value)}
          placeholder="项目目录"
        />
        <button type="button" className="secondary" onClick={pickDir}>
          选择目录
        </button>
        <button type="button" onClick={onScan} disabled={loading || !rootPath}>
          {loading ? "扫描中…" : "扫描进程"}
        </button>
      </div>
      {message && <div className="feedback-banner">{message}</div>}
      {rows.length === 0 && !loading && (
        <div className="card">
          <div className="empty">无匹配进程。</div>
        </div>
      )}
      {rows.length > 0 && (
        <div className="env-candidate-list">
          {rows.map((r) => (
            <div key={`${r.pid}-${r.snapshot_digest}`} className="card env-candidate">
              <div className="env-candidate-head">
                <span className="env-tag">PID {r.pid}</span>
                <span className="env-source">层级 {r.match_level}</span>
                {r.protection.is_protected ? (
                  <span className="env-badge err">受保护</span>
                ) : (
                  <span className="env-badge ok">可终止</span>
                )}
              </div>
              <div className="env-path">{r.name}</div>
              <div className="env-meta">
                <span className="env-meta-label">工作目录</span>
                <span className="env-version" title={r.working_directory ?? ""}>
                  {r.working_directory
                    ? formatDisplayPath(r.working_directory)
                    : "UNKNOWN"}
                </span>
              </div>
              {r.ports.length > 0 && (
                <div className="env-meta">
                  <span className="env-meta-label">端口</span>
                  <span className="env-version">{r.ports.join(", ")}</span>
                </div>
              )}
              {r.protection.is_protected && r.protection.reason && (
                <div className="env-reason">{r.protection.reason}</div>
              )}
              <div className="list-card-actions">
                <button
                  type="button"
                  disabled={r.protection.is_protected}
                  onClick={() => onTerminate(r, false)}
                >
                  终止
                </button>
                <button
                  type="button"
                  className="secondary"
                  disabled={r.protection.is_protected}
                  onClick={() => onTerminate(r, true)}
                >
                  强制终止
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
